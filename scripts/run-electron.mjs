// Starts the installed Electron reframe with an empty profile of its own, opens two real claude
// sessions in a folder, and keeps it idle so it can be measured. Nothing is sent to Claude.
// Usage: node run-electron.mjs <reframe repo with node_modules> <reframe.exe> <session folder> <idle seconds>
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';

const [repo, executablePath, folder, idleSeconds] = process.argv.slice(2);
const { _electron: electron } = createRequire(path.join(repo, 'package.json'))('@playwright/test');

const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-measure-'));
// Not inside another Claude Code, as far as the claude started here can tell.
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^CLAUDE/i.test(key) && key !== 'NO_COLOR'));
Object.assign(env, { REFRAME_USER_DATA: userData, REFRAME_LOCALE: 'en', REFRAME_DISABLE_NOTIFICATIONS: '1' });

const started = Date.now();
const app = await electron.launch({ executablePath, env });
const page = await app.firstWindow();
await page.waitForSelector('button');
console.log(`window after ${Date.now() - started} ms`);

for (const name of ['left', 'right']) {
  await page.getByRole('button', { name: 'New session' }).first().click();
  const dialog = page.getByRole('dialog');
  await dialog.getByPlaceholder('Path to a project folder').fill(folder);
  await dialog.getByPlaceholder('Shown on the tile and in notifications').fill(name);
  await dialog.getByRole('button', { name: 'Start' }).click();
  await page.locator('.tile', { has: page.locator('.tile-label', { hasText: name }) }).locator('.status-idle').waitFor({ timeout: 30_000 });
}
const pid = await app.evaluate(() => process.pid);
console.log(`READY pid=${pid}`);
await new Promise((resolve) => setTimeout(resolve, Number(idleSeconds) * 1000));

// The app may be gone before Playwright hears about it; either way it is closing.
const closed = app.waitForEvent('close', { timeout: 15_000 }).catch(() => {});
await app.evaluate(({ app: a }) => a.quit()).catch(() => {});
await closed;
await new Promise((resolve) => setTimeout(resolve, 3000));
fs.rmSync(userData, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
console.log('closed');
