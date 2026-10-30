// Smoke test against the real claude CLI in its fullscreen mode ("tui": "fullscreen"), which reads the mouse and
// pastes on a right-click of its own: in reframe a right-click must then paste once (Claude Code's paste), and
// Shift+right-click is reframe's. Opt-in like real-claude.spec.ts (REFRAME_E2E_REAL=1, REFRAME_E2E_FOLDER=<trusted
// folder>), skipped without the fullscreen setting. Who pasted is told apart by the clipboards: reframe's view of it
// is faked (test hook), the system clipboard, which Claude Code reads, is set for the test and put back afterwards
// (text only). No prompt is sent. Screenshots land in test-results/real-fullscreen-*.png.
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test, type ElectronApplication } from '@playwright/test';
import { callMain, launch, quit, tileByLabel } from './helpers';

const folder = process.env.REFRAME_E2E_FOLDER ?? process.cwd();
const settings = (): { tui?: string } => {
  try {
    return JSON.parse(fs.readFileSync(path.join(os.homedir(), '.claude', 'settings.json'), 'utf8')) as { tui?: string };
  } catch {
    return {};
  }
};

test.skip(!process.env.REFRAME_E2E_REAL, 'set REFRAME_E2E_REAL=1 to run against the real claude CLI');
test.skip(process.platform !== 'win32', 'the system clipboard is set with PowerShell here');
test.skip(settings().tui !== 'fullscreen', 'needs "tui": "fullscreen" in ~/.claude/settings.json');

const powershell = (command: string, input?: string) =>
  execFileSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', command], { encoding: 'utf8', input });
const readClipboard = () =>
  powershell('[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-Clipboard -Raw').replace(/\r?\n$/, '');
const writeClipboard = (text: string) =>
  powershell(
    '[Console]::InputEncoding = [Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())',
    text,
  );
const count = (text: string, needle: string) => text.split(needle).length - 1;

test("real claude, fullscreen: a right-click pastes once (Claude Code's), Shift+right-click is reframe's", async () => {
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-real-'));
  // Not inside another Claude Code, as far as the claude started here can tell.
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key, value]) => !/^CLAUDE/i.test(key) && value !== undefined),
  ) as Record<string, string>;
  Object.assign(env, {
    REFRAME_E2E: '1',
    REFRAME_USER_DATA: userData,
    REFRAME_LOCALE: 'en',
    REFRAME_DISABLE_NOTIFICATIONS: '1',
  });
  const before = readClipboard();
  const claudes = `CLAUDE-PASTE-${crypto.randomUUID().slice(0, 8)}`;
  writeClipboard(claudes);
  let app: ElectronApplication | null = null;
  try {
    const launched = await launch(env);
    app = launched.app;
    const { page } = launched;
    await page.setViewportSize({ width: 1400, height: 820 });
    await page.getByRole('button', { name: 'New session' }).first().click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('Path to a project folder').fill(folder);
    await dialog.getByPlaceholder('Shown on the tile and in notifications').fill('fullscreen');
    await dialog.getByRole('button', { name: 'Start' }).click();
    const tile = tileByLabel(page, 'fullscreen');
    await expect(tile.locator('.status-idle')).toBeVisible({ timeout: 60_000 });
    // The TUI draws itself and turns the mouse on.
    await page.waitForTimeout(4000);
    await page.screenshot({ path: 'test-results/real-fullscreen-0-started.png' });

    // Copies are caught here (the system clipboard stays as set above).
    await page.evaluate(() => {
      Object.defineProperty(navigator.clipboard, 'writeText', {
        value: async (text: string) => void ((window as unknown as { copied: string }).copied = text),
      });
    });
    const screen = (await tile.locator('.xterm-screen').boundingBox())!;
    const spot = { x: screen.x + screen.width / 2, y: screen.y + screen.height * 0.85 };
    /** What the terminal shows: all of it selected (Shift+drag, which xterm keeps from the program) and copied. */
    const shown = async () => {
      await page.evaluate(() => void ((window as unknown as { copied?: string }).copied = undefined));
      await page.keyboard.down('Shift');
      await page.mouse.move(screen.x + 2, screen.y + 2);
      await page.mouse.down();
      await page.mouse.move(screen.x + screen.width - 2, screen.y + screen.height - 2, { steps: 8 });
      await page.mouse.up();
      await page.keyboard.up('Shift');
      await page.keyboard.press('Control+Shift+C');
      return (await page.evaluate(() => (window as unknown as { copied?: string }).copied)) ?? '';
    };

    await callMain(app, 'fakeClipboard', { text: 'REFRAME-PASTE', image: false, files: [] });
    await page.mouse.click(spot.x, spot.y, { button: 'right' });
    await page.waitForTimeout(2000);
    await page.screenshot({ path: 'test-results/real-fullscreen-1-right-click.png' });
    let text = await shown();
    expect(count(text, claudes), `after a right-click the screen shows:\n${text}`).toBe(1);
    expect(count(text, 'REFRAME-PASTE'), `after a right-click the screen shows:\n${text}`).toBe(0);

    await page.keyboard.down('Shift');
    await page.mouse.click(spot.x, spot.y, { button: 'right' });
    await page.keyboard.up('Shift');
    await page.waitForTimeout(2000);
    await page.screenshot({ path: 'test-results/real-fullscreen-2-shift-right-click.png' });
    text = await shown();
    expect(count(text, claudes), `after Shift+right-click the screen shows:\n${text}`).toBe(1);
    expect(count(text, 'REFRAME-PASTE'), `after Shift+right-click the screen shows:\n${text}`).toBe(1);
  } finally {
    if (app) {
      await callMain(app, 'fakeClipboard', null).catch(() => {});
      await quit(app).catch(() => {});
    }
    if (before) writeClipboard(before);
    try {
      fs.rmSync(userData, { recursive: true, force: true });
    } catch {
      // still in use for a moment; the temp folder is cleaned up by the system
    }
  }
});
