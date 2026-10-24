// Ends a claude that runs outside reframe the way its user would: Ctrl+C twice in its terminal. Only when
// that does not work is the process tree killed. Used before reframe continues the session itself, so the
// same conversation never runs in two places.
import { consoleHelper } from './console';
import type { LogFn } from './log';
import { isAlive, killTree } from './process';

export type StopResult = 'exited' | 'killed' | 'failed';

export async function stopExternalClaude(pid: number, log: LogFn): Promise<StopResult> {
  if (!isAlive(pid)) return 'exited';
  if (process.platform === 'win32') {
    const typed = await typeCtrlC(pid);
    if (!typed.ok) log('warn', 'could not type Ctrl+C into the other terminal', { pid, error: typed.error });
  } else {
    for (let i = 0; i < 2; i++) {
      try {
        process.kill(pid, 'SIGINT');
      } catch {
        break;
      }
      await sleep(700);
    }
  }
  if (await exitsWithin(pid, 6000)) return 'exited';
  log('warn', 'claude in the other terminal did not exit on Ctrl+C, killing it', { pid });
  await killTree(pid);
  return (await exitsWithin(pid, 3000)) ? 'killed' : 'failed';
}

async function typeCtrlC(pid: number): Promise<{ ok: boolean; error?: string }> {
  const result = await consoleHelper({ type: 'interrupt', pid, times: 2, intervalMs: 700 });
  return result.ok ? { ok: true } : result;
}

async function exitsWithin(pid: number, ms: number): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (!isAlive(pid)) return true;
    await sleep(200);
  }
  return !isAlive(pid);
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
