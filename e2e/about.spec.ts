// End to end: About shows the version, the license and where reframe comes from; its links open in the browser
// (caught here), and the language chosen there applies at once, everywhere, and after a restart.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, quit, root, setup } from './helpers';

test('About: version, license, links, and the language', async () => {
  const { dir, env, state } = setup();
  const version = (JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')) as { version: string }).version;
  try {
    let { app, page } = await launch(env);
    await app.evaluate(({ shell }) => {
      const opened: string[] = [];
      Object.assign(globalThis, { opened });
      shell.openExternal = async (url: string) => void opened.push(url);
    });

    await page.getByRole('button', { name: 'About reframe' }).click();
    const about = page.getByRole('dialog', { name: 'About reframe' });
    await expect(about).toContainText(`Version ${version}`);
    await expect(about).toContainText('Made with ❤️ in Austria by reframe Team aka TheyCallMeJuzou');
    await about.getByText('License (MIT)').click();
    await expect(about.locator('pre')).toContainText('MIT License');
    await expect(about.locator('pre')).toContainText('reframe Team');

    await about.getByText('reframe on GitHub').click();
    await about.getByText('TheyCallMeJuzou').click();
    expect(await app.evaluate(() => (globalThis as unknown as { opened: string[] }).opened)).toEqual([
      'https://github.com/TheyCallMeJuzou/reframe',
      'https://github.com/TheyCallMeJuzou',
    ]);

    // German, right away (also in the dialog itself) and after a restart.
    await about.getByRole('combobox').selectOption('de');
    await expect(page.getByRole('dialog', { name: 'Über reframe' })).toBeVisible();
    await expect(page.locator('.toolbar').getByRole('button', { name: 'Neue Session' })).toBeVisible();
    await expect.poll(() => state().preferences.language).toBe('de');
    await quit(app);

    ({ app, page } = await launch(env));
    await expect(page.locator('.toolbar').getByRole('button', { name: 'Neue Session' })).toBeVisible();
    // Back to the system's language (English in the tests).
    await page.getByRole('button', { name: 'Über reframe' }).click();
    await page.getByRole('dialog').getByRole('combobox').selectOption('system');
    await expect(page.locator('.toolbar').getByRole('button', { name: 'New session' })).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: 'Close' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await quit(app);
  } catch (error) {
    await captureWindows('about');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
