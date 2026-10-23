// Disposable terminal view: everything stateful lives in the PTY host.
const MIME = 'application/x-reframe-tile';
const winId = Number(new URLSearchParams(location.search).get('win'));
const header = document.getElementById('header');
const termEl = document.getElementById('term');
const placeholder = document.getElementById('placeholder');

let term = null;
let port = null;
let observer = null;

function showPlaceholder(on) {
  placeholder.classList.toggle('hidden', !on);
  termEl.classList.toggle('hidden', on);
  header.classList.toggle('hidden', on);
}

function openTerminal(p) {
  disposeTerminal();
  showPlaceholder(false);
  port = p;
  term = new Terminal({
    allowProposedApi: true,
    fontFamily: '"Cascadia Mono", Consolas, monospace',
    fontSize: 13,
    cursorBlink: true,
    theme: { background: '#1e1e1e' },
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new Unicode11Addon.Unicode11Addon());
  term.unicode.activeVersion = '11';
  term.open(termEl);
  try {
    term.loadAddon(new WebglAddon.WebglAddon());
  } catch (e) {
    console.warn('webgl unavailable, using DOM renderer', e);
  }

  port.onmessage = (e) => {
    const msg = e.data;
    if (msg.type === 'snapshot') {
      term.resize(msg.cols, msg.rows);
      term.write(msg.data, () => sendFit());
    }
    if (msg.type === 'data') term.write(msg.data);
    if (msg.type === 'exit') term.write(`\r\n\x1b[90m[claude beendet, exit ${msg.exitCode}]\x1b[0m\r\n`);
  };
  term.onData((data) => port?.postMessage({ type: 'input', data }));

  const sendFit = () => {
    if (!term) return;
    fit.fit();
    port?.postMessage({ type: 'resize', cols: term.cols, rows: term.rows });
  };
  observer = new ResizeObserver(() => sendFit());
  observer.observe(termEl);
  term.focus();
}

function disposeTerminal() {
  observer?.disconnect();
  observer = null;
  port?.close();
  port = null;
  term?.dispose();
  term = null;
}

window.addEventListener('message', (e) => {
  if (e.data === 'terminal-port' && e.ports[0]) openTerminal(e.ports[0]);
});
spike.on('detach', () => {
  disposeTerminal();
  showPlaceholder(true);
});
spike.on('owner', ({ owner, me }) => {
  if (owner !== me && !term) showPlaceholder(true);
});

// --- HTML5 DnD: source side ---
header.addEventListener('dragstart', (e) => {
  e.dataTransfer.setData(MIME, JSON.stringify({ sessionId: 'tile-1', fromWin: winId }));
  e.dataTransfer.effectAllowed = 'move';
  spike.send('drag-start', { fromWin: winId });
});
header.addEventListener('dragend', (e) => {
  spike.send('drag-end', { dropEffect: e.dataTransfer.dropEffect });
});

// --- HTML5 DnD: target side (any window, including other BrowserWindows) ---
let depth = 0;
document.addEventListener('dragenter', (e) => {
  if (!e.dataTransfer.types.includes(MIME)) return;
  depth++;
  document.body.classList.add('dragover');
});
document.addEventListener('dragleave', () => {
  depth = Math.max(0, depth - 1);
  if (depth === 0) document.body.classList.remove('dragover');
});
document.addEventListener('dragover', (e) => {
  if (!e.dataTransfer.types.includes(MIME)) return;
  e.preventDefault();
  e.dataTransfer.dropEffect = 'move';
});
document.addEventListener('drop', (e) => {
  depth = 0;
  document.body.classList.remove('dragover');
  const raw = e.dataTransfer.getData(MIME);
  if (!raw) return;
  e.preventDefault();
  spike.send('tile-dropped', { targetWin: winId, data: JSON.parse(raw) });
});
