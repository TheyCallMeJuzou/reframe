// End to end: renaming a session in place, in its tile (double-click or the tile menu) and in the session list
// (its menu). Escape keeps the old name, an empty name goes back to the automatic one, and names survive a
// restart.
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('renaming sessions in the tile and in the list', async () => {
  const { dir, project, env, byName } = setup();
  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const id = byName('alpha').id;
    const label = page.locator('.tile .tile-label');

    // Double-click the name in the tile.
    await label.dblclick();
    await page.locator('.tile-label-input').fill('Backend');
    await page.keyboard.press('Enter');
    await expect(label).toHaveText('Backend');

    // The tile's menu, then Escape: nothing changes.
    await page.locator('.tile-header').click({ button: 'right' });
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Rename')).toBe(true);
    await page.locator('.tile-label-input').fill('Not this');
    await page.keyboard.press('Escape');
    await expect(label).toHaveText('Backend');

    // From the session list's menu, edited in the row.
    await page.getByRole('button', { name: 'Sessions', exact: true }).click();
    const row = page.locator(`.sidebar-row[data-session="${id}"]`);
    await row.click({ button: 'right' });
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Rename')).toBe(true);
    await page.locator('.sidebar-name-input').fill('API work');
    await page.keyboard.press('Enter');
    await expect(label).toHaveText('API work');
    await expect(row).toContainText('API work');

    // The name comes back after a restart.
    await quit(app);
    ({ app, page } = await launch(env));
    await expect(tileByLabel(page, 'API work')).toBeVisible();

    // Empty: back to the automatic name (here the one claude was started with).
    await page.locator('.tile .tile-label').dblclick();
    await page.locator('.tile-label-input').fill('');
    await page.keyboard.press('Enter');
    await expect(page.locator('.tile .tile-label')).toHaveText('alpha');
    expect(byName('alpha')).toBeUndefined(); // no name of its own any more
    await quit(app);
  } catch (error) {
    await captureWindows('rename');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
