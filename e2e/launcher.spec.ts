// End to end (Windows): claude behind a batch file reframe does not know (no npm or pnpm shim), in a folder with a
// space in its name. It runs through cmd.exe, whose process the pty's is then; its status still comes through
// (its registry entry found by its session id), and it ends with reframe as usual.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, root, setup, tileByLabel } from './helpers';

test('claude behind an unknown launcher in a folder with a space runs, with its status', async () => {
  test.skip(process.platform !== 'win32', 'batch files are Windows only');
  const { dir, project, env, launches, byName } = setup();
  try {
    const tools = path.join(dir, 'My Tools');
    fs.mkdirSync(tools);
    const launcher = path.join(tools, 'claude.cmd');
    const fake = path.join(root, 'test', 'fake-claude', 'fake-claude.mjs');
    fs.writeFileSync(launcher, `@"${process.execPath}" "${fake}" %*\r\n`);

    const { app, page } = await launch({ ...env, REFRAME_CLAUDE_BIN: launcher });
    await newSession(page, project, 'my task');
    const start = launches().find((l) => l.event === 'start')!;
    // The session's process is claude's own (for cleaning up after a crash), not cmd's, once its entry was seen.
    await expect.poll(() => byName('my task').pid, { timeout: 10_000 }).toBe(start.pid);
    expect(start.args).toEqual(
      expect.arrayContaining(['--session-id', byName('my task').claudeSessionId, '--name', 'my task']),
    );

    await tileByLabel(page, 'my task').locator('.tile-body').click();
    await page.keyboard.type('hello');
    await page.keyboard.press('Enter');
    await expect.poll(() => byName('my task').used, { timeout: 10_000 }).toBe(true);
    await quit(app);
    expect(launches().filter((l) => l.event === 'exit')).toHaveLength(1);
  } catch (error) {
    await captureWindows('launcher');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
