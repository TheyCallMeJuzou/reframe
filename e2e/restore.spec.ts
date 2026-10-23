// End to end: real app, fake claude. Creates sessions, collapses one, changes a session id with
// /clear, quits through the real shutdown path and checks that a restart restores the layout, resumes
// the used session under its latest id and starts the never-used one fresh under its own id.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { _electron as electron, expect, test, type ElectronApplication, type Page } from '@playwright/test';

const root = path.resolve(__dirname, '..');

interface SavedSession {
  id: string;
  name?: string;
  claudeSessionId: string;
  lifecycle: string;
  used?: boolean;
}

interface SavedState {
  sessions: Record<string, SavedSession>;
  windows: { hidden: boolean; dock: { sessionId: string }[] }[];
}

interface FakeLaunch {
  pid: number;
  event: 'start' | 'exit';
  args: string[];
}

function setup() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-e2e-'));
  const project = path.join(dir, 'project');
  fs.mkdirSync(project);
  const env = {
    ...process.env,
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

// Chromium helper processes can hold the profile for a few seconds after the app exits. Cleanup never
// throws, so a failing assertion is what gets reported.
async function removeWhenUnlocked(dir: string): Promise<void> {
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

let running: ElectronApplication | null = null;

async function launch(env: Record<string, string>): Promise<{ app: ElectronApplication; page: Page }> {
  const app = await electron.launch({ args: [root], env, cwd: root });
  running = app;
  const page = await app.firstWindow();
  await page.waitForSelector('.app');
  return { app, page };
}

async function quit(app: ElectronApplication): Promise<void> {
  const closed = app.waitForEvent('close');
  await app.evaluate(({ app: electronApp }) => electronApp.quit());
  await closed;
  running = null;
}

const tileByLabel = (page: Page, label: string) =>
  page.locator('.tile', { has: page.locator('.tile-label', { hasText: label }) });

async function newSession(page: Page, folder: string, name: string): Promise<void> {
  await page.getByRole('button', { name: 'New session' }).first().click();
  const dialog = page.getByRole('dialog');
  await dialog.getByPlaceholder('Path to a project folder').fill(folder);
  await dialog.getByPlaceholder('Shown on the tile and in notifications').fill(name);
  await dialog.getByRole('button', { name: 'Start' }).click();
  await expect(dialog).toBeHidden();
  await expect(tileByLabel(page, name).locator('.status-idle')).toBeVisible({ timeout: 15_000 });
}

test('sessions survive a restart with layout, dock and latest session ids', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  try {
    // --- first run ---
    let { app, page } = await launch(env);
    await expect(page.getByText('No sessions yet')).toBeVisible();

    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await expect(page.locator('.tile')).toHaveCount(2);

    // Closing the window only hides it; the sessions keep running and the hidden state is saved.
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.close());
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.isVisible())).toBe(false);
    await expect.poll(() => state().windows[0]!.hidden, { timeout: 5_000 }).toBe(true);
    expect(launches().filter((l) => l.event === 'exit')).toHaveLength(0);
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.show());
    await expect.poll(() => state().windows[0]!.hidden, { timeout: 5_000 }).toBe(false);

    // Collapse beta into the dock.
    await tileByLabel(page, 'beta').getByRole('button', { name: 'Collapse to the side bar' }).click();
    await expect(page.locator('.tile')).toHaveCount(1);
    await expect(page.locator('.dock-item', { hasText: 'beta' })).toBeVisible();

    // /clear in alpha gives it a new Claude session id; reframe must pick that up.
    const alphaId = byName('alpha');
    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    await page.keyboard.type('/clear');
    await page.keyboard.press('Enter');
    await expect
      .poll(() => state().sessions[alphaId.id]!.claudeSessionId, { timeout: 10_000 })
      .not.toBe(alphaId.claudeSessionId);
    const alphaAfterClear = state().sessions[alphaId.id]!.claudeSessionId;
    expect(state().sessions[alphaId.id]!.used).toBe(false);

    // A prompt gives the new session a transcript, so it must be resumed later.
    await page.keyboard.type('hello');
    await page.keyboard.press('Enter');
    await expect.poll(() => state().sessions[alphaId.id]!.used, { timeout: 10_000 }).toBe(true);
    const betaId = byName('beta').claudeSessionId;

    await quit(app);

    // Shutdown ended both fake processes gracefully (Ctrl+C twice), nothing is left running.
    const firstRun = launches();
    expect(firstRun.filter((l) => l.event === 'start')).toHaveLength(2);
    expect(firstRun.filter((l) => l.event === 'exit')).toHaveLength(2);
    const saved = state();
    expect(saved.windows[0]!.dock.map((d) => saved.sessions[d.sessionId]!.name)).toEqual(['beta']);
    expect(Object.values(saved.sessions).every((s) => s.lifecycle === 'open')).toBe(true);

    // --- second run ---
    ({ app, page } = await launch(env));
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect(page.locator('.dock-item', { hasText: 'beta' })).toBeVisible();

    // alpha has a transcript and is resumed; beta never got a prompt, so it starts fresh under its id.
    const relaunched = launches()
      .slice(firstRun.length)
      .filter((l) => l.event === 'start')
      .map((l) => l.args.slice(0, 2).join(' '));
    expect(relaunched.sort()).toEqual([`--resume ${alphaAfterClear}`, `--session-id ${betaId}`].sort());

    // Expanding beta brings it back next to alpha.
    await page.locator('.dock-item', { hasText: 'beta' }).click();
    await expect(page.locator('.tile')).toHaveCount(2);
    await expect(tileByLabel(page, 'beta').locator('.status-idle')).toBeVisible();

    await quit(app);
    const all = launches();
    expect(all.filter((l) => l.event === 'exit')).toHaveLength(all.filter((l) => l.event === 'start').length);
  } finally {
    await running?.close().catch(() => {});
    running = null;
    await removeWhenUnlocked(dir);
  }
});
