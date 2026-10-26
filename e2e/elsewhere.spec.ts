// End to end: one of reframe's sessions that someone resumed in a terminal of their own (claude --resume <id>, here
// the fake claude in its own pseudo console) is not run a second time. Opened from the list, it is taken over after
// asking; on reframe's start, with nobody to ask, its tile says so and waits for "Take over".
import fs from 'node:fs';
import path from 'node:path';
import * as pty from 'node-pty';
import { expect, test, type Page } from '@playwright/test';
import {
  answerDialogs,
  captureWindows,
  cleanup,
  dialogsAsked,
  launch,
  newSession,
  quit,
  setup,
  tileByLabel,
} from './helpers';

const row = (page: Page, text: string) => page.locator('.sidebar-row', { hasText: text });

/** Resumes a session in a terminal of its own, outside reframe; the promise settles when that claude has ended. */
function resumeOutside(env: Record<string, string>, dir: string, project: string, claudeSessionId: string) {
  const noHooks = path.join(dir, 'no-hooks.json');
  fs.writeFileSync(noHooks, '{}');
  const claude = pty.spawn(
    process.execPath,
    [env.REFRAME_CLAUDE_BIN!, '--resume', claudeSessionId, '--settings', noHooks],
    {
      cwd: project,
      env,
      cols: 100,
      rows: 30,
    },
  );
  let ended = false;
  claude.onExit(() => (ended = true));
  return { ended: () => ended, kill: () => !ended && claude.kill(), pid: claude.pid };
}

test('a session resumed in another terminal is taken over, not run twice', async () => {
  const { dir, project, env, launches, byName, running } = setup();
  let outside: ReturnType<typeof resumeOutside> | null = null;
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    await page.keyboard.type('hello');
    await page.keyboard.press('Enter');
    await expect.poll(() => byName('alpha').used, { timeout: 10_000 }).toBe(true);
    await tileByLabel(page, 'alpha').getByRole('button', { name: 'End session' }).click();
    await expect.poll(running, { timeout: 10_000 }).toBe(0);

    outside = resumeOutside(env, dir, project, byName('alpha').claudeSessionId);
    await expect.poll(running, { timeout: 10_000 }).toBe(1);
    await page.getByRole('button', { name: 'Sessions' }).click();
    // In reframe's list as running elsewhere, and not a second time among the other terminals' sessions.
    await expect(row(page, 'alpha')).toContainText('running elsewhere', { timeout: 10_000 });
    await expect(page.locator('.sidebar-row')).toHaveCount(1);

    await answerDialogs(app, 0);
    await row(page, 'alpha').click();
    await expect.poll(outside.ended, { timeout: 15_000 }).toBe(true);
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    expect(await dialogsAsked(app)).toEqual(['alpha is running in another terminal.']);
    await expect.poll(running).toBe(1);
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(3);
    await quit(app);
  } catch (error) {
    await captureWindows('elsewhere');
    throw error;
  } finally {
    outside?.kill();
    await cleanup(dir);
  }
});

test('a session resumed elsewhere before reframe starts waits for "Take over"', async () => {
  const { dir, project, env, byName, running } = setup();
  let outside: ReturnType<typeof resumeOutside> | null = null;
  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    await page.keyboard.type('hello');
    await page.keyboard.press('Enter');
    await expect.poll(() => byName('alpha').used, { timeout: 10_000 }).toBe(true);
    await quit(app);

    outside = resumeOutside(env, dir, project, byName('alpha').claudeSessionId);
    await expect.poll(running, { timeout: 10_000 }).toBe(1);
    ({ app, page } = await launch(env));
    const tile = tileByLabel(page, 'alpha');
    await expect(tile.getByText('This session is running in another terminal.')).toBeVisible({ timeout: 15_000 });
    expect(outside.ended()).toBe(false);
    expect(byName('alpha').lifecycle).toBe('open');

    await answerDialogs(app, 0);
    await tile.getByRole('button', { name: 'Take over' }).click();
    await expect.poll(outside.ended, { timeout: 15_000 }).toBe(true);
    await expect(tile.locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect.poll(running).toBe(1);
    await quit(app);
  } catch (error) {
    await captureWindows('elsewhere-start');
    throw error;
  } finally {
    outside?.kill();
    await cleanup(dir);
  }
});
