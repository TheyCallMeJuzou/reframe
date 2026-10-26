// End to end: sessions that need the user (here the fake claude's "/ask", like a permission prompt). The window's
// title says how many of its sessions do, and Ctrl+Shift+Space brings up the next one, also one collapsed into the
// side bar.
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('the title counts the sessions that need you, and Ctrl+Shift+Space goes to the next', async () => {
  const { dir, project, env } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await expect(page).toHaveTitle('reframe');
    const ask = async (label: string) => {
      await tileByLabel(page, label).locator('.tile-body').click();
      await page.keyboard.type('/ask');
      await page.keyboard.press('Enter');
      await expect(tileByLabel(page, label).locator('.status-waiting')).toBeVisible();
    };
    const focused = () =>
      page.evaluate(() => document.activeElement?.closest('.tile')?.querySelector('.tile-label')?.textContent ?? null);

    await ask('beta');
    await expect(page).toHaveTitle('reframe – a session needs you');
    // From alpha, which needs nothing, the shortcut goes to beta; answered there, nobody needs you any more.
    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    expect(await focused()).toBe('alpha');
    await page.keyboard.press('Control+Shift+Space');
    await expect.poll(focused).toBe('beta');
    await page.keyboard.press('Enter');
    await expect(tileByLabel(page, 'beta').locator('.status-idle')).toBeVisible();
    await expect(page).toHaveTitle('reframe');

    // Both need you, one of them collapsed: the shortcut brings that one back, and goes round between them.
    await ask('alpha');
    await ask('beta');
    await expect(page).toHaveTitle('reframe – 2 sessions need you');
    await tileByLabel(page, 'alpha').getByRole('button', { name: 'Collapse to the side bar' }).click();
    await expect(page.locator('.dock-item', { hasText: 'alpha' })).toBeVisible();
    await tileByLabel(page, 'beta').locator('.tile-body').click();
    await page.keyboard.press('Control+Shift+Space');
    await expect(page.locator('.dock-item')).toHaveCount(0);
    await expect.poll(focused).toBe('alpha');
    await page.keyboard.press('Control+Shift+Space');
    await expect.poll(focused).toBe('beta');
    await quit(app);
  } catch (error) {
    await captureWindows('needs-you');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
