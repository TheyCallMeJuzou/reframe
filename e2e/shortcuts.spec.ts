// End to end: keyboard shortcuts move the focus between tiles (which shows it with an accent frame), collapse
// and bring back tiles, toggle the session list, open the "New session" dialog and new, empty windows, which
// come back after a restart.
import { expect, test, type Page } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, pageOf, quit, setup, tileByLabel } from './helpers';

/** Label of the tile that has the keyboard focus. */
const focused = (page: Page) =>
  page.evaluate(() => document.activeElement?.closest('.tile')?.querySelector('.tile-label')?.textContent ?? null);

test('keyboard shortcuts and new windows', async () => {
  const { dir, project, env, state } = setup();
  try {
    const first = await launch(env);
    let app = first.app;
    const page = first.page;
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await newSession(page, project, 'gamma');

    // Ctrl+1…9 by position (alpha left, beta and gamma stacked right), Ctrl+Shift+arrows to the neighbor.
    await page.keyboard.press('ControlOrMeta+1');
    await expect.poll(() => focused(page)).toBe('alpha');
    await expect(tileByLabel(page, 'alpha')).toHaveCSS('border-color', 'rgb(217, 119, 87)');
    await expect(tileByLabel(page, 'beta')).not.toHaveCSS('border-color', 'rgb(217, 119, 87)');
    await page.keyboard.press('ControlOrMeta+2');
    await expect.poll(() => focused(page)).toBe('beta');
    await page.keyboard.press('ControlOrMeta+Shift+ArrowDown');
    await expect.poll(() => focused(page)).toBe('gamma');
    await page.keyboard.press('ControlOrMeta+Shift+ArrowLeft');
    await expect.poll(() => focused(page)).toBe('alpha');

    // Ctrl+Shift+M collapses the focused tile; pressed again (no tile focused), it brings it back, focused.
    await page.keyboard.press('ControlOrMeta+3');
    await expect.poll(() => focused(page)).toBe('gamma');
    await page.keyboard.press('ControlOrMeta+Shift+M');
    await expect(page.locator('.dock-item', { hasText: 'gamma' })).toBeVisible();
    await expect(page.locator('.tile')).toHaveCount(2);
    await page.keyboard.press('ControlOrMeta+Shift+M');
    await expect(page.locator('.tile')).toHaveCount(3);
    await expect.poll(() => focused(page)).toBe('gamma');

    // Ctrl+Shift+L: the session list, with the search box ready to type in.
    await page.keyboard.press('ControlOrMeta+Shift+L');
    await expect(page.getByPlaceholder('Search sessions')).toBeFocused();
    await page.keyboard.press('ControlOrMeta+Shift+L');
    await expect(page.locator('.sidebar')).toHaveCount(0);

    // Ctrl+Shift+T: the "New session" dialog.
    await page.keyboard.press('ControlOrMeta+Shift+T');
    await expect(page.getByRole('dialog')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);

    // Ctrl+Shift+N: a new, empty window, which comes back after a restart.
    const opened = app.waitForEvent('window');
    await page.keyboard.press('ControlOrMeta+Shift+N');
    const second = await opened;
    await expect(second.getByText('No sessions yet')).toBeVisible();
    await expect.poll(() => state().windows.length).toBe(2);
    await quit(app);

    const mainId = state().windows[0]!.id;
    app = (await launch(env)).app;
    await expect.poll(() => app.windows().length, { timeout: 10_000 }).toBe(2);
    await expect((await pageOf(app, mainId)).locator('.tile')).toHaveCount(3);
    await quit(app);
  } catch (error) {
    await captureWindows('shortcuts');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

test('F11: full screen, kept after a restart', async () => {
  const { dir, env, state } = setup();
  try {
    let { app, page } = await launch(env);
    const fullscreen = () => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.isFullScreen());
    await page.keyboard.press('F11');
    await expect.poll(fullscreen, { timeout: 10_000 }).toBe(true);
    await expect.poll(() => state().windows[0]!.fullscreen).toBe(true);
    await quit(app);

    ({ app, page } = await launch(env));
    await expect.poll(fullscreen, { timeout: 10_000 }).toBe(true);
    await page.keyboard.press('F11');
    await expect.poll(fullscreen, { timeout: 10_000 }).toBe(false);
    await expect.poll(() => state().windows[0]!.fullscreen).toBe(false);
    await quit(app);
  } catch (error) {
    await captureWindows('fullscreen');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
