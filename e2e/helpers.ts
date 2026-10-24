// Shared setup for end-to-end tests: the real app with a fake claude in a throwaway profile.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, type ElectronApplication, type Page } from '@playwright/test';
import type { ReframeApi } from '../src/shared/ipc';

export const root = path.resolve(__dirname, '..');

export interface SavedSession {
  id: string;
  name?: string;
  claudeSessionId: string;
  lifecycle: string;
  used?: boolean;
}

export interface SavedWindow {
  id: string;
  hidden: boolean;
  layout: unknown;
  dock: { sessionId: string }[];
}

export interface SavedState {
  sessions: Record<string, SavedSession>;
  windows: SavedWindow[];
  preferences: { nameFromFolder: boolean };
}

export interface FakeLaunch {
  pid: number;
  event: 'start' | 'exit';
  args: string[];
}

export function setup() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-e2e-'));
  const project = path.join(dir, 'project');
  fs.mkdirSync(project);
  const env = {
    ...process.env,
    REFRAME_E2E: '1',
    REFRAME_USER_DATA: path.join(dir, 'user-data'),
    REFRAME_CLAUDE_HOME: path.join(dir, 'claude-home'),
    REFRAME_CLAUDE_BIN: path.join(root, 'test', 'fake-claude', 'fake-claude.mjs'),
    // A real node: electron.exe is a GUI program on Windows and gets no pseudo-console stdio.
    REFRAME_NODE: process.execPath,
    REFRAME_FAKE_LOG: path.join(dir, 'fake.log'),
    REFRAME_LOCALE: 'en',
    REFRAME_DISABLE_NOTIFICATIONS: '1',
  } as Record<string, string>;
  const launches = (): FakeLaunch[] =>
    fs.existsSync(env.REFRAME_FAKE_LOG!)
      ? fs
          .readFileSync(env.REFRAME_FAKE_LOG!, 'utf8')
          .trim()
          .split('\n')
          .map((l) => JSON.parse(l) as FakeLaunch)
      : [];
  const state = (): SavedState => JSON.parse(fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'state.json'), 'utf8'));
  const byName = (name: string): SavedSession => Object.values(state().sessions).find((s) => s.name === name)!;
  return { dir, project, env, launches, state, byName };
}

let running: ElectronApplication | null = null;

export async function launch(env: Record<string, string>): Promise<{ app: ElectronApplication; page: Page }> {
  const app = await electron.launch({ args: [root], env, cwd: root });
  running = app;
  // Surface renderer errors in the test output; a crashed React tree otherwise just shows a blank window.
  const watch = (page: Page) => {
    page.on('pageerror', (error) => console.log(`[renderer error] ${error.stack ?? error.message}`));
    page.on('console', (message) => {
      if (message.type() === 'error') console.log(`[renderer console] ${message.text()}`);
    });
  };
  app.windows().forEach(watch);
  app.on('window', watch);
  const page = await app.firstWindow();
  await page.waitForSelector('.app');
  return { app, page };
}

/** Quits through the app's real shutdown path (as the tray's "Quit" does). */
export async function quit(app: ElectronApplication): Promise<void> {
  const closed = app.waitForEvent('close');
  await app.evaluate(({ app: electronApp }) => electronApp.quit());
  await closed;
  running = null;
}

/** For `catch`: screenshots of every open window, to see what the app showed when a test failed. */
export async function captureWindows(name: string): Promise<void> {
  for (const [i, page] of (running?.windows() ?? []).entries()) {
    await page.screenshot({ path: `test-results/${name}-window-${i + 1}.png` }).catch(() => {});
  }
}

/** For `finally`: closes a still running app and removes the profile without masking test failures. */
export async function cleanup(dir: string): Promise<void> {
  await running?.close().catch(() => {});
  running = null;
  // Chromium helper processes can hold the profile for a few seconds after the app exits.
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      fs.rmSync(dir, { recursive: true, force: true });
      return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  console.warn(`could not remove ${dir}`);
}

export const tileByLabel = (page: Page, label: string) =>
  page.locator('.tile', { has: page.locator('.tile-label', { hasText: label }) });

export async function newSession(page: Page, folder: string, name: string): Promise<void> {
  await page.getByRole('button', { name: 'New session' }).first().click();
  const dialog = page.getByRole('dialog');
  await dialog.getByPlaceholder('Path to a project folder').fill(folder);
  await dialog.getByPlaceholder('Shown on the tile and in notifications').fill(name);
  await dialog.getByRole('button', { name: 'Start' }).click();
  await expect(dialog).toBeHidden();
  await expect(tileByLabel(page, name).locator('.status-idle')).toBeVisible({ timeout: 15_000 });
}

/** The page of the window with this id. */
export async function pageOf(app: ElectronApplication, windowId: string): Promise<Page> {
  for (let attempt = 0; attempt < 50; attempt++) {
    for (const page of app.windows()) {
      const id = await page
        .evaluate(() => (globalThis as unknown as { reframe: ReframeApi }).reframe.getView().then((v) => v?.windowId))
        .catch(() => undefined);
      if (id === windowId) return page;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`no page for window ${windowId}`);
}
