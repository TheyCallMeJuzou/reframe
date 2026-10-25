// End to end (Windows): a session in a WSL folder runs its claude in that distribution through wsl.exe (here a
// stand-in, with folders for distributions). No hooks reach reframe from WSL, so its status comes from Claude Code's
// registry there; the tile shows the folder as "Fake ~/proj"; dropped files and a pasted image come in as Linux
// paths; and it resumes after a restart. A distribution without claude says so.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, root, setup, tileByLabel } from './helpers';

test.skip(process.platform !== 'win32', 'WSL is Windows only');

function wslSetup() {
  const base = setup();
  const share = path.join(base.dir, 'wsl');
  const folder = path.join(share, 'Fake', 'home', 'ann', 'proj');
  fs.mkdirSync(folder, { recursive: true });
  fs.mkdirSync(path.join(share, 'NoClaude', 'home', 'ann', 'proj'), { recursive: true });
  fs.writeFileSync(path.join(share, 'NoClaude', 'no-claude'), '');
  base.env.REFRAME_WSL_EXE = path.join(root, 'test', 'fake-wsl', 'fake-wsl.mjs');
  base.env.REFRAME_WSL_SHARE = share;
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
