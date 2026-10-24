// End to end: started by the system (--autostart), reframe stays in the tray: no window shows, but every
// session resumes in the background, and the windows come back as they were.
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

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

    // Shown from the tray (or by starting reframe again), the window is there with its session.
    await callMain(app, 'revealWindows');
    await expect.poll(visible).toEqual([true]);
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await quit(app);
  } catch (error) {
    await captureWindows('autostart');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
