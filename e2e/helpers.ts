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
  fullscreen?: boolean;
  workspaceId?: string;
  layout: unknown;
  dock: { sessionId: string }[];
}

export interface SavedState {
  sessions: Record<string, SavedSession>;
  windows: SavedWindow[];
  preferences: { nameFromFolder: boolean; language: 'system' | 'en' | 'de'; fontSize: number };
  workspaces: { id: string; name: string }[];
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
  userData = env.REFRAME_USER_DATA!;
  const state = (): SavedState => JSON.parse(fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'state.json'), 'utf8'));
  const byName = (name: string): SavedSession => Object.values(state().sessions).find((s) => s.name === name)!;
  // Fake claudes that are up: each writes its registry entry once it reads input (and so handles Ctrl+C), and
  // removes it when it exits. One ended while still starting leaves no "exit" in the launch log.
  const registry = path.join(env.REFRAME_CLAUDE_HOME!, 'sessions');
  const running = (): number => (fs.existsSync(registry) ? fs.readdirSync(registry).length : 0);
  return { dir, project, env, launches, state, byName, running };
}

let running: ElectronApplication | null = null;
/** User data folder of the latest setup(), for the app log of a failed test. */
let userData: string | null = null;

export async function launch(
  env: Record<string, string>,
  args: string[] = [],
): Promise<{ app: ElectronApplication; page: Page }> {
  // REFRAME_E2E_APP: run against a packaged reframe (e.g. dist/win-unpacked/reframe.exe) instead of the dev build.
  const packaged = process.env.REFRAME_E2E_APP;
  const app = await electron.launch(
    packaged ? { executablePath: packaged, args, env, cwd: root } : { args: [root, ...args], env, cwd: root },
  );
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
  // From the event loop, as a click on the tray's "Quit" would (not from inside the debugger's evaluate call).
  await app.evaluate(({ app: electronApp }) => void setTimeout(() => electronApp.quit(), 0));
  await closed;
  running = null;
}

/** For `catch`: screenshots of every open window and the app log, to see what happened when a test failed. */
export async function captureWindows(name: string): Promise<void> {
  for (const [i, page] of (running?.windows() ?? []).entries()) {
    await page.screenshot({ path: `test-results/${name}-window-${i + 1}.png` }).catch(() => {});
  }
  const log = userData && path.join(userData, 'logs', 'main.log');
  if (!log || !fs.existsSync(log)) return;
  const text = fs.readFileSync(log, 'utf8');
  console.log(`--- main.log (${name})\n${text}`);
  // On GitHub also as an annotation (the end of it), readable without access to the job log.
  if (process.env.GITHUB_ACTIONS) {
    const tail = text.split('\n').slice(-40).join('\n');
    const escape = (value: string) => value.replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A');
    console.log(`::warning title=main.log (${name})::${escape(tail)}`);
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

type Point = { x: number; y: number };
type MainHook = { reframe: Record<string, (...args: never[]) => unknown> };

/** Calls a public method of the app in the main process (REFRAME_E2E exposes it as globalThis.reframe). */
export function callMain<T>(app: ElectronApplication, method: string, ...args: unknown[]): Promise<T> {
  return app.evaluate(
    (_electron, a) => {
      const reframe = (globalThis as unknown as MainHook).reframe;
      return (reframe[a.method] as (...params: unknown[]) => unknown).apply(reframe, a.args);
    },
    { method, args },
  ) as Promise<T>;
}

/** What a drag that ends outside every window does: the tile or group gets a window at `point`. */
export const tearOutNode = (app: ElectronApplication, key: string, point: Point) =>
  callMain<boolean>(app, 'tearOutNode', key, point);

/** What a drop of another window's tile or group does in `page`'s window. */
export const transferNode = (
  page: Page,
  key: string,
  fromWindowId: string,
  target: { key: string; side: 'left' | 'right' | 'top' | 'bottom' } | null,
) =>
  page.evaluate((a) => (globalThis as unknown as { reframe: ReframeApi }).reframe.transferNode(a), {
    key,
    fromWindowId,
    target,
  });

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
