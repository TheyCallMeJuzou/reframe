// Runs one request in the console helper (a short-lived utilityProcess, Windows only).
import path from 'node:path';
import { utilityProcess } from 'electron';
import type { ConsoleRequest, ConsoleResponse } from '../consolehelper/protocol';

export function consoleHelper(request: ConsoleRequest, timeoutMs = 10_000): Promise<ConsoleResponse> {
  if (process.platform !== 'win32') return Promise.resolve({ ok: false, error: 'consoles exist on Windows only' });
  return new Promise((resolve) => {
    const helper = utilityProcess.fork(path.join(__dirname, 'consolehelper.js'), [], {
      serviceName: 'reframe-console-helper',
    });
    const timer = setTimeout(() => {
      helper.kill();
      resolve({ ok: false, error: 'console helper timed out' });
    }, timeoutMs);
    helper.once('message', (result: ConsoleResponse) => {
      clearTimeout(timer);
      resolve(result);
    });
    helper.once('exit', () => {
      clearTimeout(timer);
      resolve({ ok: false, error: 'console helper exited' });
    });
    helper.postMessage(request);
  });
}
