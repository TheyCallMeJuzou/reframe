// End to end: Ctrl with = (or +) and - change the terminals' font size, Ctrl+0 brings it back to normal, and
// Ctrl+mouse wheel over a tile does the same. The size is a preference, kept across restarts.
import { expect, test, type Page } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

/** The font size the terminal draws at. */
const measured = (page: Page) => page.locator('.terminal-host').first().getAttribute('data-font-size');

test('text size: keys and mouse wheel, kept after a restart', async () => {
  const { dir, project, env, state } = setup();
  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const body = tileByLabel(page, 'alpha').locator('.tile-body');
    await body.click();
    expect(await measured(page)).toBe('13');

    // Two quick presses add up.
    await page.keyboard.press('Control+Equal');
    await page.keyboard.press('Control+Equal');
    await expect.poll(() => measured(page)).toBe('15');
    await expect.poll(() => state().preferences.fontSize).toBe(15);
    await page.keyboard.press('Control+Minus');
    await expect.poll(() => measured(page)).toBe('14');

    // A wheel notch away from you, with Ctrl, over the tile.
    const box = (await body.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -100);
    await page.keyboard.up('Control');
    await expect.poll(() => measured(page)).toBe('15');
    await expect.poll(() => state().preferences.fontSize).toBe(15);
    await quit(app);

    ({ app, page } = await launch(env));
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    expect(await measured(page)).toBe('15');
    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    await page.keyboard.press('Control+0');
    await expect.poll(() => measured(page)).toBe('13');
    await expect.poll(() => state().preferences.fontSize).toBe(13);
    await quit(app);
  } catch (error) {
    await captureWindows('text-size');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
