// Smoke test against the real claude CLI (no prompts are sent). Opt-in, because it needs Claude Code
// installed and logged in: REFRAME_E2E_REAL=1 REFRAME_E2E_FOLDER=<trusted folder> npx playwright test real
// Screenshots land in test-results/real-*.png.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test, type Page } from '@playwright/test';

const root = path.resolve(__dirname, '..');
const folder = process.env.REFRAME_E2E_FOLDER ?? process.cwd();

test.skip(!process.env.REFRAME_E2E_REAL, 'set REFRAME_E2E_REAL=1 to run against the real claude CLI');

const tileByLabel = (page: Page, label: string) =>
  page.locator('.tile', { has: page.locator('.tile-label', { hasText: label }) });

test('real claude: start two sessions, restart, both resume', async () => {
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-real-'));
  const env = { ...process.env, REFRAME_USER_DATA: userData, REFRAME_LOCALE: 'de' } as Record<string, string>;

  let app = await electron.launch({ args: [root], env, cwd: root });
  let page = await app.firstWindow();
  await page.setViewportSize({ width: 1400, height: 820 });

  for (const name of ['links', 'rechts']) {
    await page.getByRole('button', { name: 'Neue Session' }).first().click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('Pfad zu einem Projektordner').fill(folder);
    await dialog.getByPlaceholder('Steht auf der Kachel und in Benachrichtigungen').fill(name);
    if (name === 'links') await page.screenshot({ path: 'test-results/real-1-dialog.png' });
    await dialog.getByRole('button', { name: 'Starten' }).click();
    await expect(tileByLabel(page, name).locator('.status-idle')).toBeVisible({ timeout: 30_000 });
  }
  await page.waitForTimeout(2500);
  await page.screenshot({ path: 'test-results/real-2-two-sessions.png' });

  await tileByLabel(page, 'rechts').getByRole('button', { name: 'In die Seitenleiste einklappen' }).click();
  await page.waitForTimeout(800);
  await page.screenshot({ path: 'test-results/real-3-collapsed.png' });

  let closed = app.waitForEvent('close');
  await app.evaluate(({ app: a }) => a.quit());
  await closed;

  app = await electron.launch({ args: [root], env, cwd: root });
  page = await app.firstWindow();
  await page.setViewportSize({ width: 1400, height: 820 });
  await expect(tileByLabel(page, 'links').locator('.status-idle')).toBeVisible({ timeout: 30_000 });
  await page.locator('.dock-item', { hasText: 'rechts' }).click();
  await expect(tileByLabel(page, 'rechts').locator('.status-idle')).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(2500);
  await page.screenshot({ path: 'test-results/real-4-after-restart.png' });

  closed = app.waitForEvent('close');
  await app.evaluate(({ app: a }) => a.quit());
  await closed;
  await new Promise((resolve) => setTimeout(resolve, 3000));
  fs.rmSync(userData, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
});
