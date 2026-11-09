// End to end: the session list shows when each session was last used. A prompt (and Claude's answer) counts, and
// puts the session first; starting reframe again, which resumes every session, does not. A past session opened from
// the list keeps the time it was last used.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

const HOUR = 3_600_000;

test('the session list shows when each session was last used', async () => {
  const { dir, project, env, state, byName } = setup();
  const stateFile = path.join(env.REFRAME_USER_DATA!, 'state.json');
  // A past session, as Claude Code leaves it, last written 5 hours ago.
  const transcripts = path.join(env.REFRAME_CLAUDE_HOME!, 'projects', 'fake-project');
  fs.mkdirSync(transcripts, { recursive: true });
  const pastId = crypto.randomUUID();
  const pastFile = path.join(transcripts, `${pastId}.jsonl`);
  const lines = [
    { type: 'user', cwd: project, message: { content: 'some work' } },
    { type: 'ai-title', aiTitle: 'Past task' },
  ];
  fs.writeFileSync(pastFile, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  const fiveHoursAgo = new Date(Date.now() - 5 * HOUR);
  fs.utimesSync(pastFile, fiveHoursAgo, fiveHoursAgo);

  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    // A prompt and its answer are a use.
    const before = byName('alpha').lastActiveAt;
    await tileByLabel(page, 'alpha').locator('.tile-body').click();
    await page.keyboard.type('hello');
    await page.keyboard.press('Enter');
    await expect.poll(() => byName('alpha').lastActiveAt).toBeGreaterThan(before);
    await quit(app);

    // alpha used 3 hours ago, beta 2 days ago.
    const saved = state();
    const id = (name: string) => Object.values(saved.sessions).find((s) => s.name === name)!.id;
    saved.sessions[id('alpha')]!.lastActiveAt = Date.now() - 3 * HOUR;
    saved.sessions[id('beta')]!.lastActiveAt = Date.now() - 48 * HOUR;
    fs.writeFileSync(stateFile, JSON.stringify(saved, null, 2));

    // Started again, both resume: that is not using them.
    ({ app, page } = await launch(env));
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect(tileByLabel(page, 'beta').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await page.getByRole('button', { name: 'Sessions', exact: true }).click();
    const appRows = page.locator('.sidebar-row[data-session]');
    const used = (name: string) => page.locator(`.sidebar-row[data-session="${id(name)}"] .sidebar-used`);
    await expect(used('alpha')).toHaveText('3h');
    await expect(used('beta')).toHaveText('2d');
    await expect(appRows.first()).toHaveAttribute('data-session', id('alpha'));
    await expect(page.locator(`.sidebar-row[data-session="${id('alpha')}"]`)).toHaveAttribute('title', /\nLast used /);

    // A prompt in beta: just used, and first.
    await tileByLabel(page, 'beta').locator('.tile-body').click();
    await page.keyboard.type('again');
    await page.keyboard.press('Enter');
    await expect(used('beta')).toHaveText('now');
    await expect(appRows.first()).toHaveAttribute('data-session', id('beta'));
    await expect(used('alpha')).toHaveText('3h');

    // The past session shows when it was last written, and keeps that once opened here.
    const pastRow = page.locator('.sidebar-row', { hasText: 'Past task' });
    await expect(pastRow.locator('.sidebar-used')).toHaveText('5h');
    await pastRow.click();
    const adopted = () => Object.values(state().sessions).find((s) => s.claudeSessionId === pastId);
    await expect.poll(() => adopted()?.lastActiveAt).toBeLessThan(Date.now() - 4 * HOUR);
    await expect(page.locator(`.sidebar-row[data-session="${adopted()!.id}"] .sidebar-used`)).toHaveText('5h');
    await quit(app);
  } catch (error) {
    await captureWindows('last-used');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
