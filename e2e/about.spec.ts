// End to end: About shows the version, the license and where reframe comes from; its links open in the browser
// (caught here).
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, quit, root, setup } from './helpers';

test('About: version, license and links', async () => {
  const { dir, env } = setup();
  const version = (JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')) as { version: string }).version;
  try {
    const { app, page } = await launch(env);
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

    await about.getByRole('button', { name: 'Close' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await quit(app);
  } catch (error) {
    await captureWindows('about');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
