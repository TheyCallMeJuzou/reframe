// End to end: a tile fills the window for a while (Ctrl+Shift+Z or its button) and gives it back. The layout itself
// stays as it is, and a new tile shows all of them again.
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('Ctrl+Shift+Z lets a tile fill the window and back, the layout unchanged', async () => {
  const { dir, project, env, state } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    const layout = JSON.stringify(state().windows[0]!.layout);
    const area = (await page.locator('.layout').boundingBox())!;
    const before = (await tileByLabel(page, 'alpha').boundingBox())!;

    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    await page.keyboard.press('Control+Shift+Z');
    await expect(page.locator('.tile')).toHaveCount(1);
    const zoomed = (await tileByLabel(page, 'alpha').boundingBox())!;
    expect(zoomed.width).toBeGreaterThan(area.width - 20);
    expect(zoomed.height).toBeGreaterThan(area.height - 20);
    // Typing goes on where it was.
    await expect(tileByLabel(page, 'alpha').locator('.xterm-helper-textarea')).toBeFocused();
    expect(JSON.stringify(state().windows[0]!.layout)).toBe(layout);

    await page.keyboard.press('Control+Shift+Z');
    await expect(page.locator('.tile')).toHaveCount(2);
    const after = (await tileByLabel(page, 'alpha').boundingBox())!;
    expect(Math.abs(after.width - before.width)).toBeLessThan(2);

    // The tile's button does it too; a tile added meanwhile shows them all again.
    await tileByLabel(page, 'beta').getByRole('button', { name: 'Fill the window' }).click();
    await expect(page.locator('.tile')).toHaveCount(1);
    await expect(tileByLabel(page, 'beta').getByRole('button', { name: 'Show all tiles again' })).toBeVisible();
    await newSession(page, project, 'gamma');
    await expect(page.locator('.tile')).toHaveCount(3);
    // And it does not come back by itself once the layout is as it was when zoomed.
    await tileByLabel(page, 'gamma').getByRole('button', { name: 'End session' }).click();
    await expect(page.locator('.tile')).toHaveCount(2);
    await quit(app);
  } catch (error) {
    await captureWindows('zoom');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
