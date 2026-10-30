// Smoke test against Claude Code in a real WSL distribution (Windows). Opt-in, because it needs Claude Code installed
// and logged in there, a folder it trusts, and sends one short prompt; the second test needs Windows Terminal, where
// it opens a window of its own:
//   REFRAME_E2E_WSL=<distribution> REFRAME_E2E_WSL_FOLDER=<Linux path of a trusted folder> npx playwright test real-wsl
// Screenshots land in test-results/real-wsl-*.png.
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test, type ElectronApplication, type Page } from '@playwright/test';

const root = path.resolve(__dirname, '..');
const distro = process.env.REFRAME_E2E_WSL ?? '';
const linuxFolder = process.env.REFRAME_E2E_WSL_FOLDER ?? '';
const folder = `\\\\wsl.localhost\\${distro}${linuxFolder.replace(/\//g, '\\')}`;

test.skip(!distro || process.platform !== 'win32', 'set REFRAME_E2E_WSL=<distribution> to run against Claude in WSL');

const tileByLabel = (page: Page, label: string) =>
  page.locator('.tile', { has: page.locator('.tile-label', { hasText: label }) });

/** A test hook of reframe's main process (REFRAME_E2E). */
const callMain = <T>(app: ElectronApplication, method: string, ...args: unknown[]) =>
  app.evaluate(
    (_electron, a) => {
      const reframe = (globalThis as unknown as { reframe: Record<string, (...params: unknown[]) => unknown> }).reframe;
      return reframe[a.method]!.apply(reframe, a.args);
    },
    { method, args },
  ) as Promise<T>;

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

test('real claude typed into a WSL terminal tab: dropped onto reframe, taken over, the tab closed', async () => {
  test.setTimeout(180_000);
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-real-wsl-'));
  const env = {
    ...process.env,
    REFRAME_USER_DATA: userData,
    REFRAME_LOCALE: 'en',
    REFRAME_E2E: '1',
    REFRAME_DISABLE_NOTIFICATIONS: '1',
  } as Record<string, string>;
  const sessionId = crypto.randomUUID();
  /** The wsl.exe processes (System32's and the one it starts) running this test's terminal tab, or reframe's. */
  const wslPids = () =>
    execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `(Get-CimInstance Win32_Process -Filter "Name='wsl.exe'" | Where-Object { $_.CommandLine -like '*${sessionId}*' }).ProcessId`,
      ],
      { encoding: 'utf8' },
    )
      .split(/\s+/)
      .filter(Boolean)
      .map(Number);
  const alive = (pid: number) => {
    try {
      process.kill(pid, 0);
      return true;
    } catch {
      return false;
    }
  };

  // A Windows Terminal window of its own (never a tab in one of the user's): bash in the distribution, claude typed
  // into it, the shell left at its prompt once claude ends. (No ";": Windows Terminal splits its command line there.)
  spawn(
    'wt.exe',
    ['-w', 'new', 'wsl.exe', '-d', distro, '--cd', linuxFolder, '--', 'bash', '-lic'].concat(
      `claude --session-id ${sessionId} --name dropped || true && exec bash -l`,
    ),
    { stdio: 'ignore', detached: true },
  );
  let app: ElectronApplication | null = null;
  try {
    await expect.poll(() => wslPids().length, { timeout: 20_000 }).toBeGreaterThan(0);
    const terminal = wslPids();
    app = await electron.launch({ args: [root], env, cwd: root });
    const page = await app.firstWindow();
    await page.setViewportSize({ width: 1200, height: 760 });
    const hwnd = await callMain<string | null>(app, 'terminalWindowOf', terminal[0]);
    expect(hwnd).toBeTruthy();
    const bounds = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getContentBounds());
    const point = { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 };

    // Dropped once claude is up over there (registered): reframe finds it in that window, by the wsl.exe of its tab.
    await expect
      .poll(() => callMain<boolean>(app!, 'dropTerminalWindow', hwnd, point), { timeout: 60_000, intervals: [2000] })
      .toBe(true);
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('dropped', { timeout: 30_000 });
    await expect(dialog.locator('.takeover-item')).toHaveCount(1);
    await page.screenshot({ path: 'test-results/real-wsl-4-dropped.png' });
    await dialog.getByRole('button', { name: 'Take over' }).click();

    const tile = tileByLabel(page, 'dropped');
    await expect(tile.locator('.status-idle')).toBeVisible({ timeout: 60_000 });
    await expect(tile.locator('.tile-cwd')).toContainText(distro);
    // claude ended there, its shell got "exit", and the tab (with its window) went with wsl.exe.
    await expect.poll(() => terminal.filter(alive).length, { timeout: 20_000 }).toBe(0);
    const log = fs.readFileSync(path.join(userData, 'logs', 'main.log'), 'utf8');
    expect(log).toContain('terminal of a claude in wsl found');
    expect(log).toContain('"shell":"wsl"');
    await page.waitForTimeout(1500);
    await page.screenshot({ path: 'test-results/real-wsl-5-taken-over.png' });

    const closed = app.waitForEvent('close');
    await app.evaluate(({ app: a }) => a.quit());
    await closed;
    app = null;
  } finally {
    if (app) await app.close().catch(() => {});
    // Whatever is left of this test's claudes, in its terminal or in reframe.
    for (const pid of wslPids()) spawn('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore' });
    spawnSync('wsl.exe', ['-d', distro, '-e', 'pkill', '-f', sessionId], { stdio: 'ignore' });
    await new Promise((resolve) => setTimeout(resolve, 3000));
    fs.rmSync(userData, { recursive: true, force: true, maxRetries: 10, retryDelay: 500 });
  }
});
