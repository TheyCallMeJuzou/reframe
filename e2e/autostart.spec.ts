// End to end: started by the system (--autostart), reframe stays in the tray: no window shows, but every
// session resumes in the background. Starting reframe again (e.g. from the Start menu) does not start a second
// one: the running one shows its windows, as they were.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
// In Node, the electron package is just the path to Electron's executable.
import electronPath from 'electron';
import { captureWindows, cleanup, launch, newSession, quit, root, setup, tileByLabel } from './helpers';

/** Starts reframe once more, as a user would, and resolves when that process is gone again. */
function startAgain(env: Record<string, string>): Promise<number | null> {
  const packaged = process.env.REFRAME_E2E_APP;
  const child = spawn(packaged ?? (electronPath as unknown as string), packaged ? [] : [root], {
    env,
    cwd: root,
    stdio: 'ignore',
  });
  return new Promise((resolve) => child.on('exit', resolve));
}

test('started by the system: windows stay hidden, sessions resume', async () => {
  const { dir, project, env, launches } = setup();
  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await quit(app);

    ({ app, page } = await launch(env, ['--autostart']));
    const visible = () => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().map((w) => w.isVisible()));
    await expect(tileByLabel(page, 'alpha')).toHaveCount(1);
    expect(await visible()).toEqual([false]);
    await expect.poll(() => launches().filter((l) => l.event === 'start').length).toBe(2);

    // Started again: the second process leaves, and the window is there with its session.
    await startAgain(env);
    await expect.poll(visible).toEqual([true]);
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    const log = fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'logs', 'main.log'), 'utf8');
    expect(log).toContain('reframe already runs, showing its windows');
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(2);
    await quit(app);
  } catch (error) {
    await captureWindows('autostart');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
