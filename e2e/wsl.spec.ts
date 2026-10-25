// End to end (Windows): a session in a WSL folder runs its claude in that distribution through wsl.exe (here a
// stand-in, with folders for distributions). No hooks reach reframe from WSL, so its status comes from Claude Code's
// registry there; the tile shows the folder as "Fake ~/proj"; dropped files and a pasted image come in as Linux
// paths; and it resumes after a restart. A distribution without claude says so. The session list shows the
// sessions of running distributions (and says which known one is not running, with a button to start it), and a
// session running in a WSL terminal is taken over.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import * as pty from 'node-pty';
import { expect, test, type Page } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test.skip(process.platform !== 'win32', 'WSL is Windows only');

/** The stand-in WSL (from setup()) with two distributions: Fake, and NoClaude without claude. */
function wslSetup() {
  const base = setup();
  const share = base.env.REFRAME_WSL_SHARE!;
  const folder = path.join(share, 'Fake', 'home', 'ann', 'proj');
  fs.mkdirSync(folder, { recursive: true });
  fs.mkdirSync(path.join(share, 'NoClaude', 'home', 'ann', 'proj'), { recursive: true });
  fs.writeFileSync(path.join(share, 'NoClaude', 'no-claude'), '');
  return { ...base, share, folder };
}

test('a session in a WSL folder runs there: status, Linux paths, resume', async () => {
  const { dir, env, share, folder, launches, byName } = wslSetup();
  try {
    let { app, page } = await launch(env);
    await newSession(page, folder, 'linux');
    const tile = tileByLabel(page, 'linux');
    await expect(tile.locator('.tile-cwd')).toHaveText('Fake ~/proj');
    const start = launches().find((l) => l.event === 'start')!;
    expect(start.args).not.toContain('--settings');

    const transcript = () =>
      path.join(
        share,
        'Fake',
        'home',
        'ann',
        '.claude',
        'projects',
        'fake-project',
        `${byName('linux').claudeSessionId}.jsonl`,
      );
    const prompts = () =>
      fs.existsSync(transcript())
        ? fs
            .readFileSync(transcript(), 'utf8')
            .trim()
            .split('\n')
            .map((line) => (JSON.parse(line) as { prompt: string }).prompt.trim().replace(/^'|'$/g, ''))
        : [];

    // A file dropped onto the tile (one that exists only in memory is saved on C:) is typed in as /mnt/c/….
    const body = tile.locator('.tile-body');
    await body.evaluate((el) => {
      const data = new DataTransfer();
      data.items.add(new File(['hello'], 'note.txt', { type: 'text/plain' }));
      el.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: data }));
      el.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: data }));
    });
    await page.waitForTimeout(500);
    await page.keyboard.press('Enter');
    await expect.poll(prompts).toHaveLength(1);
    expect(prompts()[0]).toMatch(/^\/mnt\/c\/.*\/note\.txt$/);
    // Busy, then done: announced from the registry (the tests turn notifications off, the log says which one).
    const log = () => fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'logs', 'main.log'), 'utf8');
    await expect.poll(() => log().includes('"kind":"done"'), { timeout: 10_000 }).toBe(true);

    // Claude Code in WSL cannot read the clipboard: Ctrl+V with an image types in a saved copy of it.
    await callMain(app, 'fakeClipboard', { text: '', image: true, files: [] });
    await page.keyboard.press('Control+V');
    await page.waitForTimeout(500);
    await page.keyboard.press('Enter');
    await expect.poll(prompts).toHaveLength(2);
    expect(prompts()[1]).toMatch(/^\/mnt\/c\/.*\/clipboard\.png$/);
    await callMain(app, 'fakeClipboard', null);

    // Used, as the registry told (no hook did), so it resumes after a restart.
    await expect.poll(() => byName('linux').used).toBe(true);
    await quit(app);
    ({ app, page } = await launch(env));
    await expect(tileByLabel(page, 'linux').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    const starts = launches().filter((l) => l.event === 'start');
    expect(starts).toHaveLength(2);
    expect(starts[1]!.args).toContain('--resume');
    // The tile was there before the distribution answered and the session started; it is connected all the same.
    await tileByLabel(page, 'linux').locator('.tile-body').click();
    await page.keyboard.type('after the restart');
    await page.keyboard.press('Enter');
    await expect.poll(prompts).toHaveLength(3);
    expect(prompts()[2]).toBe('after the restart');
    await quit(app);
  } catch (error) {
    await captureWindows('wsl');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

test('a WSL folder whose distribution has no claude says so', async () => {
  const { dir, env, share } = wslSetup();
  try {
    const { app, page } = await launch(env);
    await page.getByRole('button', { name: 'New session' }).first().click();
    const dialog = page.getByRole('dialog');
    await dialog.getByPlaceholder('Path to a project folder').fill(path.join(share, 'NoClaude', 'home', 'ann', 'proj'));
    await dialog.getByRole('button', { name: 'Start' }).click();
    await expect(dialog.locator('.form-error')).toHaveText(
      'Claude Code was not found in NoClaude (WSL). Install it there: claude.com/claude-code',
    );
    await quit(app);
  } catch (error) {
    await captureWindows('wsl-no-claude');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

const row = (page: Page, text: string) => page.locator('.sidebar-row', { hasText: text });

test('the session list: WSL sessions, taking over one, starting a distribution', async () => {
  const { dir, env, share, launches } = wslSetup();
  let external: pty.IPty | null = null;
  let externalExited = false;
  try {
    // Known from an earlier run to have Claude Code, but not running now.
    fs.mkdirSync(path.join(share, 'Stopped', 'home', 'ann'), { recursive: true });
    fs.writeFileSync(path.join(share, 'Stopped', 'stopped'), '');
    fs.mkdirSync(env.REFRAME_USER_DATA!, { recursive: true });
    fs.writeFileSync(
      path.join(env.REFRAME_USER_DATA!, 'state.json'),
      JSON.stringify({
        version: 1,
        sessions: {},
        windows: [],
        recentFolders: [],
        preferences: { nameFromFolder: true, language: 'en', fontSize: 13 },
        workspaces: [],
        wslDistros: ['Stopped'],
      }),
    );
    // In Fake: a past session, and one running in a terminal of its own.
    const claudeHome = path.join(share, 'Fake', 'home', 'ann', '.claude');
    const pastId = crypto.randomUUID();
    fs.mkdirSync(path.join(share, 'Fake', 'home', 'ann', 'old'), { recursive: true });
    fs.mkdirSync(path.join(claudeHome, 'projects', '-home-ann-old'), { recursive: true });
    fs.writeFileSync(
      path.join(claudeHome, 'projects', '-home-ann-old', `${pastId}.jsonl`),
      [
        { type: 'user', cwd: '/home/ann/old', message: { content: 'tidy up the logs' } },
        { type: 'ai-title', aiTitle: 'Log cleanup' },
      ]
        .map((line) => `${JSON.stringify(line)}\n`)
        .join(''),
    );
    const runningId = crypto.randomUUID();
    fs.mkdirSync(path.join(share, 'Fake', 'home', 'ann', 'other'), { recursive: true });
    external = pty.spawn(
      process.execPath,
      [
        env.REFRAME_WSL_EXE!,
        '-d',
        'Fake',
        '--cd',
        '/home/ann/other',
        '--exec',
        'env',
        'PATH=/usr/bin',
        'claude',
      ].concat(['--session-id', runningId, '--name', 'outside']),
      { env, cols: 100, rows: 30 },
    );
    external.onExit(() => (externalExited = true));
    const registry = path.join(claudeHome, 'sessions');
    await expect.poll(() => (fs.existsSync(registry) ? fs.readdirSync(registry).length : 0)).toBe(1);

    const { app, page } = await launch(env);
    await page.getByRole('button', { name: 'Sessions' }).click();
    await expect(row(page, 'outside')).toContainText('other · Fake · running elsewhere', { timeout: 15_000 });
    await expect(row(page, 'Log cleanup')).toContainText('old · Fake');
    const note = page.locator('.sidebar-note');
    await expect(note).toContainText('Stopped (WSL) is not running');

    // Started from the list, it runs (kept running by reframe) and its note goes.
    await note.getByRole('button', { name: 'Start' }).click();
    await expect(note).toHaveCount(0, { timeout: 15_000 });

    // The past one resumes in its WSL folder.
    await row(page, 'Log cleanup').click();
    await expect
      .poll(() => launches().find((l) => l.event === 'start' && l.args.join(' ').includes(`--resume ${pastId}`)))
      .toBeTruthy();
    await expect(page.locator('.tile')).toHaveCount(1);
    await expect(page.locator('.tile-cwd')).toHaveText('Fake ~/old');

    // The running one is taken over: it ends over there (on a signal) and continues here, under its name.
    await app.evaluate(({ dialog }) => {
      Object.assign(dialog, { showMessageBox: async () => ({ response: 0, checkboxChecked: false }) });
    });
    await row(page, 'outside').click();
    await expect.poll(() => externalExited, { timeout: 15_000 }).toBe(true);
    await expect(tileByLabel(page, 'outside').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect(tileByLabel(page, 'outside').locator('.tile-cwd')).toHaveText('Fake ~/other');
    expect(launches().filter((l) => l.event === 'start' && l.args.includes(runningId))).toHaveLength(2);
    await quit(app);
  } catch (error) {
    await captureWindows('wsl-list');
    throw error;
  } finally {
    if (external && !externalExited) external.kill();
    await cleanup(dir);
  }
});
