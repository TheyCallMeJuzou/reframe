// Messages between main and the PTY host. Every start of a session gets a new `instance` number, so that
// events of a process that is still going away are never mistaken for those of its successor.

export type HostRequest =
  | {
      type: 'spawn';
      sessionId: string;
      instance: number;
      file: string;
      args: string[];
      cwd: string;
      env: Record<string, string>;
      cols: number;
      rows: number;
    }
  /** Carries a MessagePort for a view of the session. */
  | { type: 'attach'; sessionId: string }
  | { type: 'write'; sessionId: string; data: string }
  /** Kills the process if still running and forgets the session, if it is still that instance. */
  | { type: 'dispose'; sessionId: string; instance: number };

export type HostEvent =
  | { type: 'spawned'; sessionId: string; instance: number; pid: number }
  | { type: 'spawn-failed'; sessionId: string; instance: number; message: string }
  | { type: 'exit'; sessionId: string; instance: number; pid: number; exitCode: number }
  /** An error nobody handled; the host keeps running (ending it would end every session). */
  | { type: 'error'; message: string };
