// PTY host (Electron utilityProcess): owns the claude process and a headless xterm mirror.
// Views (renderer windows) attach via MessagePorts; on attach they get a serialized snapshot, then live data.
const pty = require('node-pty');
const { Terminal } = require('@xterm/headless');
const { SerializeAddon } = require('@xterm/addon-serialize');
const { Unicode11Addon } = require('@xterm/addon-unicode11');

let proc = null;
let mirror = null;
let serializer = null;
const views = new Map(); // port -> { live: boolean, pending: string[] }

function send(msg) {
  process.parentPort.postMessage(msg);
}

function spawn({ file, args, cwd, env, cols, rows }) {
  for (const port of views.keys()) port.close();
  views.clear();
  mirror?.dispose();
  mirror = new Terminal({ cols, rows, scrollback: 5000, allowProposedApi: true });
  serializer = new SerializeAddon();
  mirror.loadAddon(serializer);
  mirror.loadAddon(new Unicode11Addon());
  mirror.unicode.activeVersion = '11';

  proc = pty.spawn(file, args, { name: 'xterm-256color', cols, rows, cwd, env });
  send({ type: 'spawned', pid: proc.pid });

  proc.onData((data) => {
    mirror.write(data);
    for (const [port, view] of views) {
      if (view.live) port.postMessage({ type: 'data', data });
      else view.pending.push(data);
    }
  });
  proc.onExit(({ exitCode, signal }) => {
    send({ type: 'exit', exitCode, signal });
    for (const port of views.keys()) port.postMessage({ type: 'exit', exitCode });
  });
}

function attach(port) {
  const view = { live: false, pending: [] };
  views.set(port, view);
  port.on('message', (e) => {
    const msg = e.data;
    if (msg.type === 'input' && proc) proc.write(msg.data);
    if (msg.type === 'resize' && proc && msg.cols > 0 && msg.rows > 0) {
      proc.resize(msg.cols, msg.rows);
      mirror.resize(msg.cols, msg.rows);
    }
  });
  port.on('close', () => views.delete(port));
  port.start();
  // The empty write acts as a marker: its callback runs once everything received so far is parsed,
  // so the snapshot is consistent and everything after it is in view.pending.
  mirror.write('', () => {
    port.postMessage({ type: 'snapshot', data: serializer.serialize(), cols: mirror.cols, rows: mirror.rows });
    for (const data of view.pending) port.postMessage({ type: 'data', data });
    view.pending = [];
    view.live = true;
  });
}

process.parentPort.on('message', (e) => {
  const msg = e.data;
  if (msg.type === 'spawn') spawn(msg);
  if (msg.type === 'attach') attach(e.ports[0]);
  if (msg.type === 'write' && proc) proc.write(msg.data);
  if (msg.type === 'serialize') {
    mirror.write('', () => send({ type: 'serialized', reqId: msg.reqId, data: serializer.serialize(), text: plainText() }));
  }
  if (msg.type === 'kill' && proc) proc.kill();
});

function plainText() {
  const buf = mirror.buffer.active;
  const lines = [];
  for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? '');
  return lines.join('\n').replace(/\n+$/, '');
}
