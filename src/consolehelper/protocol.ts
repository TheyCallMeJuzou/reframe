// Requests to the console helper (Windows only). Each runs in a short-lived process, because attaching to
// another process's console must not happen in the main process.

export type ConsoleRequest =
  /** Types Ctrl+C into the console of `pid`, as if the user pressed it there. */
  | { type: 'interrupt'; pid: number; times: number; intervalMs: number }
  /** Types text into the console of `pid` ("\r" is Enter), e.g. "exit\r" to leave an idle shell. */
  | { type: 'type'; pid: number; text: string }
  /** For each pid: its console window and that window's owner (the terminal window hosting the tab). */
  | { type: 'owners'; pids: number[] }
  /** Names of the processes attached to the console of `pid` (shell, claude, anything else running there). */
  | { type: 'console-processes'; pid: number };

export interface ConsoleOwner {
  pid: number;
  /** Window handles as decimal strings; null when the process has no console or attaching failed. */
  consoleWindow: string | null;
  owner: string | null;
  rootOwner: string | null;
}

export interface AttachedProcess {
  pid: number;
  /** Executable file name, e.g. "pwsh.exe"; null when it could not be read. */
  name: string | null;
}

export type ConsoleResponse =
  { ok: true; owners?: ConsoleOwner[]; processes?: AttachedProcess[] } | { ok: false; error: string };
