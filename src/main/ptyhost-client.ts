// Main-process side of the PTY host. Should the host die (it runs native code: node-pty and ConPTY), its sessions
// are gone, but a new host is started right away so they can be started again; after a few deaths in a row it is
// left down instead of being restarted in a loop.
import { EventEmitter } from 'node:events';
import path from 'node:path';
import { MessageChannelMain, utilityProcess, type UtilityProcess, type WebContents } from 'electron';
import { IPC } from '@shared/ipc';
import type { HostEvent, HostRequest } from '../ptyhost/protocol';

type Events = {
  event: [HostEvent];
  /** The host died; `restarting`: a new one is being started (false once it died too often). */
  crashed: [code: number, restarting: boolean];
};

/** Deaths within this time count as a row, and how many of them are restarted. */
const RESTART_WINDOW_MS = 60_000;
const MAX_RESTARTS = 3;

export class PtyHostClient extends EventEmitter<Events> {
  private child: UtilityProcess | null = null;
  private stopped = false;
  private restarts: number[] = [];

  start(): void {
    this.stopped = false;
    this.fork();
  }

  stop(): void {
    this.stopped = true;
    const child = this.child;
    this.child = null;
    child?.kill();
  }

  get running(): boolean {
    return this.child !== null;
  }

  /** The host's process id (the end-to-end tests end it to see what a crash does). */
  get pid(): number | undefined {
    return this.child?.pid;
  }

  spawn(request: Omit<Extract<HostRequest, { type: 'spawn' }>, 'type'>): void {
    this.send({ type: 'spawn', ...request });
  }

  write(sessionId: string, data: string): void {
    this.send({ type: 'write', sessionId, data });
  }

  dispose(sessionId: string, instance: number): void {
    this.send({ type: 'dispose', sessionId, instance });
  }

  /** Connects a window to a session's terminal through a fresh MessagePort pair. */
  attach(sessionId: string, contents: WebContents): void {
    if (!this.child) return;
    const { port1, port2 } = new MessageChannelMain();
    this.child.postMessage({ type: 'attach', sessionId } satisfies HostRequest, [port1]);
    contents.postMessage(IPC.terminalPort, { sessionId }, [port2]);
  }

  private fork(): void {
    const child = utilityProcess.fork(path.join(__dirname, 'ptyhost.js'), [], { serviceName: 'reframe-pty-host' });
    this.child = child;
    child.on('message', (event: HostEvent) => this.emit('event', event));
    child.on('exit', (code) => {
      // stop() let it go, or it is an earlier host.
      if (this.child !== child) return;
      this.child = null;
      const now = Date.now();
      this.restarts = this.restarts.filter((at) => now - at < RESTART_WINDOW_MS);
      const restarting = !this.stopped && this.restarts.length < MAX_RESTARTS;
      // Told first, so the sessions of the dead host are done with before anything starts in the new one.
      this.emit('crashed', code, restarting);
      if (!restarting || this.stopped) return;
      this.restarts.push(now);
      this.fork();
    });
  }

  private send(request: HostRequest): void {
    this.child?.postMessage(request);
  }
}
