// End to end: a window whose page crashed is loaded again, with its tiles, and its sessions keep running.
// Playwright gives up on a page once it crashed, so the reloaded one is driven through its webContents.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup } from './helpers';

test('a window whose page crashed is loaded again, its sessions still running', async () => {
  const { dir, project, env, launches, byName } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    // A call that reaches the page while it is down never returns, so each one gets a second.
    const run = (code: string) =>
      Promise.race([
        app.evaluate(
          ({ BrowserWindow }, c) => BrowserWindow.getAllWindows()[0]!.webContents.executeJavaScript(c),
          code,
        ),
        new Promise((resolve) => setTimeout(() => resolve('no answer'), 1000)),
      ]);

    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.webContents.forcefullyCrashRenderer());
    await expect.poll(() => run(`!!document.querySelector('.tile .status-idle')`), { timeout: 15_000 }).toBe(true);
    const log = fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'logs', 'main.log'), 'utf8');
    expect(log).toContain('window crashed, loading it again');
    // The same claude: nothing was started again.
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(1);

    // And its terminal is connected to it again: a prompt typed there reaches it (its hook marks the session used).
    expect(byName('alpha').used).toBeFalsy();
    await run(`document.querySelector('.tile .xterm-helper-textarea').focus()`);
    await app.evaluate(({ BrowserWindow }) => {
      const contents = BrowserWindow.getAllWindows()[0]!.webContents;
      contents.insertText('hello');
      contents.sendInputEvent({ type: 'keyDown', keyCode: 'Return' });
      contents.sendInputEvent({ type: 'char', keyCode: '\r' });
      contents.sendInputEvent({ type: 'keyUp', keyCode: 'Return' });
    });
    await expect.poll(() => byName('alpha').used, { timeout: 10_000 }).toBe(true);

    await quit(app);
  } catch (error) {
    await captureWindows('renderer-crash');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
