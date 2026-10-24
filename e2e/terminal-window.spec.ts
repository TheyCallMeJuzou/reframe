// End to end (Windows): a terminal window with claude is dropped onto reframe. A fake claude runs under cmd in
// a real console window (or a Windows Terminal tab, if that is the default terminal). The test does what the
// release of a dragged window does (REFRAME_E2E hook), confirms the dialog, and checks that the session moved
// into reframe and that the console, with only cmd left, was closed by typing "exit".
import { execFileSync, spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, quit, setup, tileByLabel } from './helpers';

test.skip(process.platform !== 'win32', 'terminal window takeover is Windows only for now');

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

test('a terminal window dropped onto reframe hands its claude over and closes', async () => {
  const { dir, project, env, launches } = setup();
  const sessionId = crypto.randomUUID();
  const noHooks = path.join(dir, 'no-hooks.json');
  fs.writeFileSync(noHooks, '{}');
  // An interactive cmd in its own console window that runs claude as a command, like typing it at the prompt.
  // (`start` gives it a console; a detached spawn would leave cmd without one, and a batch file would answer
  // Ctrl+C with "Terminate batch job?".)
  const command = `"${process.execPath}" "${env.REFRAME_CLAUDE_BIN}" --session-id ${sessionId} --name outside --settings "${noHooks}"`;
  spawn('cmd.exe', ['/c', 'start', '"reframe test terminal"', 'cmd.exe', '/s', '/k', `"${command}"`], {
    cwd: project,
    env,
    stdio: 'ignore',
    windowsVerbatimArguments: true,
  });
  let shellPid = 0;
  const registry = path.join(env.REFRAME_CLAUDE_HOME!, 'sessions');
  const fakePid = () => {
    try {
      for (const file of fs.readdirSync(registry)) {
        const entry = JSON.parse(fs.readFileSync(path.join(registry, file), 'utf8'));
        if (entry.sessionId === sessionId) return entry.pid as number;
      }
    } catch {
      // not there yet
    }
    return 0;
  };
  try {
    await expect.poll(fakePid, { timeout: 20_000 }).toBeGreaterThan(0);
    const pid = fakePid();
    shellPid = Number(
      execFileSync(
        'powershell',
        ['-NoProfile', '-Command', `(Get-CimInstance Win32_Process -Filter "ProcessId=${pid}").ParentProcessId`],
        { encoding: 'utf8' },
      ).trim(),
    );
    expect(shellPid).toBeGreaterThan(0);

    const { app, page } = await launch(env);
    const hwnd = await callMain<string | null>(app, 'terminalWindowOf', pid);
    expect(hwnd).toBeTruthy();
    const bounds = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getContentBounds());
    const point = { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 };
    expect(await callMain<boolean>(app, 'dropTerminalWindow', hwnd, point)).toBe(true);

    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('Take over from the terminal');
    await expect(dialog).toContainText('outside');
    await dialog.getByRole('button', { name: 'Take over' }).click();

    // claude left the terminal and continues in reframe, under the name it had there.
    await expect.poll(() => alive(pid), { timeout: 15_000 }).toBe(false);
    await expect(tileByLabel(page, 'outside')).toBeVisible({ timeout: 15_000 });
    await expect
      .poll(() => launches().filter((l) => l.event === 'start' && l.args.join(' ').includes(sessionId)).length)
      .toBe(2);

    // Only cmd was left in that console, so reframe typed "exit" and the console went away.
    await expect.poll(() => alive(shellPid), { timeout: 15_000 }).toBe(false);
    const log = fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'logs', 'main.log'), 'utf8');
    expect(log).toContain('typing exit');

    await quit(app);
  } catch (error) {
    await captureWindows('terminal-window');
    const logFile = path.join(env.REFRAME_USER_DATA!, 'logs', 'main.log');
    if (fs.existsSync(logFile)) console.log(`--- main.log\n${fs.readFileSync(logFile, 'utf8')}`);
    throw error;
  } finally {
    if (shellPid && alive(shellPid)) spawn('taskkill', ['/PID', String(shellPid), '/T', '/F'], { stdio: 'ignore' });
    await cleanup(dir);
  }
});
