// Messages between main and the PTY host.

export type HostRequest =
  | {
      type: 'spawn';
      sessionId: string;
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
  /** Kills the process if still running and forgets the session. */
  | { type: 'dispose'; sessionId: string };

export type HostEvent =
  | { type: 'spawned'; sessionId: string; pid: number }
  | { type: 'spawn-failed'; sessionId: string; message: string }
  | { type: 'exit'; sessionId: string; pid: number; exitCode: number };
