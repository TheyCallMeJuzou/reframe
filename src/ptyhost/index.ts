// PTY host (Electron utilityProcess). Owns every claude process and a headless xterm mirror per
// session. Windows are only views: they attach through a MessagePort, get a serialized snapshot of
// the screen, then live output. Sessions survive views coming and going.
import type { MessagePortMain } from 'electron';
import * as pty from 'node-pty';
import { Terminal } from '@xterm/headless';
import { SerializeAddon } from '@xterm/addon-serialize';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import type { TerminalHostMessage, TerminalViewMessage } from '@shared/ipc';
import type { HostEvent, HostRequest } from './protocol';

interface View {
  live: boolean;
  pending: string[];
}

interface Session {
  instance: number;
  proc: pty.IPty;
  mirror: Terminal;
  serializer: SerializeAddon;
  views: Map<MessagePortMain, View>;
  exitCode: number | null;
}

const sessions = new Map<string, Session>();
/** Views that came before their session's process (one in WSL starts once its distribution answered). */
const early = new Map<string, MessagePortMain[]>();
const parent = process.parentPort;

process.on('uncaughtException', (error) => {
  emit({ type: 'error', message: error.stack ?? String(error) });
});

function emit(event: HostEvent): void {
  parent.postMessage(event);
}

function post(port: MessagePortMain, message: TerminalHostMessage): void {
  port.postMessage(message);
}

function spawn(request: Extract<HostRequest, { type: 'spawn' }>): void {
  const { sessionId, instance } = request;
  const waiting = early.get(sessionId) ?? [];
  early.delete(sessionId);
  dispose(sessionId, null);
  const mirror = new Terminal({ cols: request.cols, rows: request.rows, scrollback: 5000, allowProposedApi: true });
  const serializer = new SerializeAddon();
  mirror.loadAddon(serializer);
  mirror.loadAddon(new Unicode11Addon());
  mirror.unicode.activeVersion = '11';

  let proc: pty.IPty;
  try {
    proc = pty.spawn(request.file, request.args, {
      name: 'xterm-256color',
      cols: request.cols,
      rows: request.rows,
      cwd: request.cwd,
      env: request.env,
      // Windows: the ConPTY that comes with node-pty (as in Windows Terminal and VS Code). The one built into
      // Windows crashed this process now and then when several sessions ended at once (access violation).
      useConptyDll: true,
    });
  } catch (error) {
    mirror.dispose();
    for (const port of waiting) port.close();
    emit({ type: 'spawn-failed', sessionId, instance, message: String(error) });
    return;
  }

  const session: Session = { instance, proc, mirror, serializer, views: new Map(), exitCode: null };
  sessions.set(sessionId, session);
  emit({ type: 'spawned', sessionId, instance, pid: proc.pid });
  for (const port of waiting) attach(sessionId, port);

  proc.onData((data) => {
    mirror.write(data);
    for (const [port, view] of session.views) {
      if (view.live) post(port, { type: 'data', data });
      else view.pending.push(data);
    }
  });
  proc.onExit(({ exitCode }) => {
    session.exitCode = exitCode;
    for (const port of session.views.keys()) post(port, { type: 'exit', exitCode });
    emit({ type: 'exit', sessionId, instance, pid: proc.pid, exitCode });
  });
}

function attach(sessionId: string, port: MessagePortMain): void {
  const session = sessions.get(sessionId);
  if (!session) {
    early.set(sessionId, [...(early.get(sessionId) ?? []), port]);
    return;
  }
  const view: View = { live: false, pending: [] };
  session.views.set(port, view);
  port.on('message', (event) => {
    const message = event.data as TerminalViewMessage;
    if (session.exitCode !== null) return;
    if (message.type === 'input') session.proc.write(message.data);
    if (message.type === 'resize' && message.cols > 1 && message.rows > 1) {
      session.proc.resize(message.cols, message.rows);
      session.mirror.resize(message.cols, message.rows);
    }
  });
  port.on('close', () => session.views.delete(port));
  port.start();
  // The empty write is a marker: its callback runs once everything received so far is parsed, so the
  // snapshot is consistent and everything after it is in view.pending.
  session.mirror.write('', () => {
    if (!session.views.has(port)) return;
    post(port, {
      type: 'snapshot',
      data: session.serializer.serialize(),
      cols: session.mirror.cols,
      rows: session.mirror.rows,
    });
    for (const data of view.pending) post(port, { type: 'data', data });
    view.pending = [];
    view.live = true;
    if (session.exitCode !== null) post(port, { type: 'exit', exitCode: session.exitCode });
  });
}

/** Forgets a session; with an instance, only if it is still that one (a newer start must survive). */
function dispose(sessionId: string, instance: number | null): void {
  const session = sessions.get(sessionId);
  if (!session || (instance !== null && session.instance !== instance)) return;
  sessions.delete(sessionId);
  for (const port of session.views.keys()) port.close();
  if (session.exitCode === null) {
    try {
      session.proc.kill();
    } catch {
      // already gone
    }
  }
  session.mirror.dispose();
}

parent.on('message', (event) => {
  const request = event.data as HostRequest;
  switch (request.type) {
    case 'spawn':
      spawn(request);
      break;
    case 'attach': {
      const port = event.ports[0];
      if (port) attach(request.sessionId, port);
      break;
    }
    case 'write':
      if (sessions.get(request.sessionId)?.exitCode === null) sessions.get(request.sessionId)!.proc.write(request.data);
      break;
    case 'dispose':
      dispose(request.sessionId, request.instance);
      break;
  }
});
