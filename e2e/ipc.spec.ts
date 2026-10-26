// End to end: what a window asks main for applies to that window. A window cannot end a session another window
// shows (each tile's × asks for its own window's session).
import { expect, test } from '@playwright/test';
import type { ReframeApi } from '../src/shared/ipc';
import { callMain, captureWindows, cleanup, launch, newSession, pageOf, quit, setup, tileByLabel } from './helpers';

test("a window cannot end another window's session", async () => {
  const { dir, project, env, byName } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const other = await pageOf(app, await callMain<string>(app, 'newWindow'));
    const end = (from: typeof page) =>
      from.evaluate(
        (id) => (globalThis as unknown as { reframe: ReframeApi }).reframe.endSession(id),
        byName('alpha').id,
      );

    await end(other);
    await page.waitForTimeout(1000);
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible();
    expect(byName('alpha').lifecycle).toBe('open');

    await end(page);
    await expect(tileByLabel(page, 'alpha')).toHaveCount(0);
    await expect.poll(() => byName('alpha').lifecycle).toBe('ended');
    await quit(app);
  } catch (error) {
    await captureWindows('ipc');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
