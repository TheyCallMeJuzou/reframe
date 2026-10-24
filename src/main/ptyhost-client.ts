// Main-process side of the PTY host.
import { EventEmitter } from 'node:events';
import path from 'node:path';
import { MessageChannelMain, utilityProcess, type UtilityProcess, type WebContents } from 'electron';
import { IPC } from '@shared/ipc';
import type { HostEvent, HostRequest } from '../ptyhost/protocol';

type Events = { event: [HostEvent]; crashed: [code: number] };

export class PtyHostClient extends EventEmitter<Events> {
  private child: UtilityProcess | null = null;

  start(): void {
    this.child = utilityProcess.fork(path.join(__dirname, 'ptyhost.js'), [], { serviceName: 'reframe-pty-host' });
    this.child.on('message', (event: HostEvent) => this.emit('event', event));
    this.child.on('exit', (code) => {
      const wasRunning = this.child !== null;
      this.child = null;
      if (wasRunning) this.emit('crashed', code);
    });
  }

  stop(): void {
    const child = this.child;
    this.child = null;
    child?.kill();
  }

  get running(): boolean {
    return this.child !== null;
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

  private send(request: HostRequest): void {
    this.child?.postMessage(request);
  }
}
