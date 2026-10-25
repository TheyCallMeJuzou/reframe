// Smoke test against Claude Code in a real WSL distribution (Windows). Opt-in, because it needs Claude Code installed
// and logged in there, a folder it trusts, and sends one short prompt:
//   REFRAME_E2E_WSL=<distribution> REFRAME_E2E_WSL_FOLDER=<Linux path of a trusted folder> npx playwright test real-wsl
// Screenshots land in test-results/real-wsl-*.png.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test, type Page } from '@playwright/test';

const root = path.resolve(__dirname, '..');
const distro = process.env.REFRAME_E2E_WSL ?? '';
const folder = `\\\\wsl.localhost\\${distro}${(process.env.REFRAME_E2E_WSL_FOLDER ?? '').replace(/\//g, '\\')}`;

test.skip(!distro || process.platform !== 'win32', 'set REFRAME_E2E_WSL=<distribution> to run against Claude in WSL');

const tileByLabel = (page: Page, label: string) =>
  page.locator('.tile', { has: page.locator('.tile-label', { hasText: label }) });

test('real claude in WSL: start, a prompt, restart, resume', async () => {
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-real-wsl-'));
  const env = { ...process.env, REFRAME_USER_DATA: userData, REFRAME_LOCALE: 'en' } as Record<string, string>;
  delete env.REFRAME_DISABLE_NOTIFICATIONS;

  let app = await electron.launch({ args: [root], env, cwd: root });
  let page = await app.firstWindow();
  await page.setViewportSize({ width: 1200, height: 760 });
  await page.getByRole('button', { name: 'New session' }).first().click();
  const dialog = page.getByRole('dialog');
  await dialog.getByPlaceholder('Path to a project folder').fill(folder);
  await dialog.getByPlaceholder('Shown on the tile and in notifications').fill('wsl');
  await dialog.getByRole('button', { name: 'Start' }).click();
  const tile = tileByLabel(page, 'wsl');
  await expect(tile.locator('.status-idle')).toBeVisible({ timeout: 60_000 });
  await expect(tile.locator('.tile-cwd')).toContainText(distro);
  await page.waitForTimeout(2000);
  await page.screenshot({ path: 'test-results/real-wsl-1-started.png' });

  // One short prompt: busy, then idle again, from the registry alone.
  await tile.locator('.tile-body').click();
  await page.keyboard.type('Reply with only the word OK');
  await page.keyboard.press('Enter');
  await expect(tile.locator('.status-busy')).toBeVisible({ timeout: 15_000 });
  await expect(tile.locator('.status-idle')).toBeVisible({ timeout: 120_000 });
  await page.waitForTimeout(1500);
  await page.screenshot({ path: 'test-results/real-wsl-2-answered.png' });

  let closed = app.waitForEvent('close');
  await app.evaluate(({ app: a }) => a.quit());
  await closed;

  app = await electron.launch({ args: [root], env, cwd: root });
  page = await app.firstWindow();
  await page.setViewportSize({ width: 1200, height: 760 });
  await expect(tileByLabel(page, 'wsl').locator('.status-idle')).toBeVisible({ timeout: 60_000 });
  await page.waitForTimeout(2500);
  await page.screenshot({ path: 'test-results/real-wsl-3-resumed.png' });

  closed = app.waitForEvent('close');
  await app.evaluate(({ app: a }) => a.quit());
  await closed;
  await new Promise((resolve) => setTimeout(resolve, 3000));
  fs.rmSync(userData, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
});
