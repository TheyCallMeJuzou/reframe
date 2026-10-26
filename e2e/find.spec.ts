// End to end: Ctrl+Shift+F finds text in the focused tile's terminal. Typed and echoed by the fake claude, "zebra"
// is on the screen four times.
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('Ctrl+Shift+F finds text in a terminal, Enter goes on, Esc closes', async () => {
  const { dir, project, env } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const tile = tileByLabel(page, 'alpha');
    await tile.locator('.tile-body').click();
    await page.keyboard.type('zebra and zebra');
    await page.keyboard.press('Enter');
    await expect(tile.locator('.status-idle')).toBeVisible();

    await page.keyboard.press('Control+Shift+F');
    const field = tile.locator('.tile-find-input');
    await expect(field).toBeFocused();
    await page.keyboard.type('zebra');
    const count = tile.locator('.tile-find-count');
    await expect(count).toHaveText(/^\d of 4$/);
    const first = await count.textContent();
    await page.keyboard.press('Enter');
    await expect(count).not.toHaveText(first!);
    await expect(count).toHaveText(/^\d of 4$/);
    await page.keyboard.press('Shift+Enter');
    await expect(count).toHaveText(first!);

    await field.fill('giraffe');
    await expect(count).toHaveText('No results');
    // Esc closes it and puts the focus back into the terminal.
    await page.keyboard.press('Escape');
    await expect(tile.locator('.tile-find')).toHaveCount(0);
    await expect(tile.locator('.xterm-helper-textarea')).toBeFocused();
    await quit(app);
  } catch (error) {
    await captureWindows('find');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
