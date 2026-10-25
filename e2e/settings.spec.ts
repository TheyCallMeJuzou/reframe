// End to end: Settings (the gear in the toolbar, or Ctrl+,). The language applies at once, everywhere, and after a
// restart; the text size has buttons too; update checks can be turned off where reframe checks; and separators snap
// to a grid over the layout while dragged (12 steps), unless Alt is held or snapping is turned off.
import { expect, test, type Page } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup } from './helpers';

test('Settings: language, text size, update checks', async () => {
  const { dir, env, state } = setup();
  try {
    let { app, page } = await launch(env);
    await page.keyboard.press('Control+Comma');
    const settings = page.getByRole('dialog', { name: 'Settings' });
    await expect(settings).toBeVisible();

    // The text size by its buttons.
    await settings.getByRole('button', { name: 'Larger' }).click();
    await expect(settings.locator('output')).toHaveText('14');
    await expect.poll(() => state().preferences.fontSize).toBe(14);
    await settings.getByRole('button', { name: 'Normal' }).click();
    await expect.poll(() => state().preferences.fontSize).toBe(13);

    // Update checks: offered only where this copy checks (in the tests it does not, until it is told to).
    const updates = settings.getByRole('checkbox', { name: 'Check for updates' });
    await expect(updates).toHaveCount(0);
    await callMain(app, 'fakeUpdateChecks');
    await expect(updates).toBeChecked();
    await updates.uncheck();
    await expect.poll(() => state().preferences.checkForUpdates).toBe(false);
    await updates.check();
    await expect.poll(() => state().preferences.checkForUpdates).toBe(true);

    // German, right away (also in the dialog itself) and after a restart.
    await settings.getByRole('combobox').selectOption('de');
    await expect(page.getByRole('dialog', { name: 'Einstellungen' })).toBeVisible();
    await expect(page.locator('.toolbar').getByRole('button', { name: 'Neue Session' })).toBeVisible();
    await expect.poll(() => state().preferences.language).toBe('de');
    await quit(app);

    ({ app, page } = await launch(env));
    await expect(page.locator('.toolbar').getByRole('button', { name: 'Neue Session' })).toBeVisible();
    // Back to the system's language (English in the tests), from the gear this time.
    await page.getByRole('button', { name: 'Einstellungen' }).click();
    await page.getByRole('dialog').getByRole('combobox').selectOption('system');
    await expect(page.locator('.toolbar').getByRole('button', { name: 'New session' })).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: 'Close' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await quit(app);
  } catch (error) {
    await captureWindows('settings');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

/** Drags the first separator to `share` of the layout's width, with Alt held if asked. */
async function dragSeparator(page: Page, share: number, alt = false) {
  const area = (await page.locator('main.layout').boundingBox())!;
  const separator = (await page.locator('.separator').first().boundingBox())!;
  const y = separator.y + separator.height / 2;
  const from = separator.x + separator.width / 2;
  const to = area.x + area.width * share;
  await page.mouse.move(from, y);
  if (alt) await page.keyboard.down('Alt');
  await page.mouse.down();
  for (let i = 1; i <= 10; i++) await page.mouse.move(from + ((to - from) * i) / 10, y);
  await page.mouse.up();
  if (alt) await page.keyboard.up('Alt');
}

test('Separators snap to a grid, unless Alt is held or snapping is off', async () => {
  const { dir, project, env, state } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    // The first panel's share of the side-by-side split (saved a moment after the drag).
    const first = () => (state().windows[0]!.layout as { sizes: number[] }).sizes[0]!;
    const near = (value: number) => (share: number) => Math.abs(share - value) < 1;

    // 37 % is nearest to 4/12 of the layout: there it lands (a bit less, so the separator's middle is on the line).
    await dragSeparator(page, 0.37);
    await expect.poll(() => near(33.3)(first())).toBe(true);

    // Alt places it freely, and does not open the window's menu bar on its way up.
    await dragSeparator(page, 0.37, true);
    await expect.poll(() => near(37)(first())).toBe(true);
    const menuBar = () => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.isMenuBarVisible());
    expect(await menuBar()).toBe(false);

    // Turned off, it does not snap at all.
    await page.getByRole('button', { name: 'Settings' }).click();
    await page.getByRole('dialog').getByRole('checkbox', { name: 'Snap separators to a grid' }).uncheck();
    await expect.poll(() => state().preferences.snapToGrid).toBe(false);
    await page.getByRole('dialog').getByRole('button', { name: 'Close' }).click();
    await dragSeparator(page, 0.6);
    await expect.poll(() => near(60)(first())).toBe(true);
    await quit(app);
  } catch (error) {
    await captureWindows('snap');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
