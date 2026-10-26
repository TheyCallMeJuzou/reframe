// Process helpers. Killing the whole tree matters: claude starts children (MCP servers, tools) and a
// dying pseudo console does not reliably take them down on Windows.
import { execFile } from 'node:child_process';
import { win32 } from './win32';

export function isAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === 'EPERM';
  }
}

/**
 * When a process started, as the system tells it: on Windows its creation time, which Claude Code's registry has
 * too (procStart). Null where that cannot be told.
 */
export function processStart(pid: number): string | null {
  return win32()?.processStart(pid) ?? null;
}

export function killTree(pid: number): Promise<void> {
  if (process.platform === 'win32') {
    return new Promise((resolve) => {
      execFile('taskkill', ['/PID', String(pid), '/T', '/F'], { windowsHide: true, timeout: 10_000 }, () => resolve());
    });
  }
  try {
    process.kill(-pid, 'SIGKILL'); // node-pty starts the child as a process group leader
  } catch {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {
      // already gone
    }
  }
  return Promise.resolve();
}
