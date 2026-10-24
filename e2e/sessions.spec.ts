// End to end: the session list. Ended sessions are reopened and removed; a past session found in Claude
// Code's transcripts is resumed; a claude running outside reframe (a fake in its own pseudo console, like
// a Windows Terminal tab) is taken over: reframe types Ctrl+C into its console and continues it here.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import * as pty from 'node-pty';
import { expect, test, type Page } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

const row = (page: Page, text: string) => page.locator('.sidebar-row', { hasText: text });

test('session list: reopen, remove, resume a found session, take over a running one', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  let external: pty.IPty | null = null;
  let externalExited = false;
  try {
    // A past session (transcript only) and one running outside reframe.
    const pastId = crypto.randomUUID();
    const transcripts = path.join(env.REFRAME_CLAUDE_HOME!, 'projects', 'elsewhere');
    fs.mkdirSync(transcripts, { recursive: true });
    fs.writeFileSync(
      path.join(transcripts, `${pastId}.jsonl`),
      JSON.stringify({ type: 'user', cwd: project, message: { content: 'refactor the parser' } }) +
        '\n' +
        JSON.stringify({ type: 'ai-title', aiTitle: 'Parser refactor' }) +
        '\n',
    );
    const runningId = crypto.randomUUID();
    const noHooks = path.join(dir, 'no-hooks.json');
    fs.writeFileSync(noHooks, '{}');
    external = pty.spawn(
      process.execPath,
      [env.REFRAME_CLAUDE_BIN!, '--session-id', runningId, '--name', 'outside', '--settings', noHooks],
      { cwd: project, env, cols: 100, rows: 30 },
    );
    external.onExit(() => (externalExited = true));

    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await page.getByRole('button', { name: 'Sessions' }).click();
    await expect(row(page, 'alpha')).toContainText('window 1');
    await expect(row(page, 'Parser refactor')).toBeVisible();
    await expect(row(page, 'outside')).toContainText('running elsewhere');

    // Search filters every section.
    await page.getByPlaceholder('Search sessions').fill('parser');
    await expect(page.locator('.sidebar-row')).toHaveCount(1);
    await page.getByPlaceholder('Search sessions').fill('');

    // An ended session reopens from the list, and can be removed once it is ended again.
    await tileByLabel(page, 'beta').getByRole('button', { name: 'End session' }).click();
    await expect(row(page, 'beta')).toContainText('ended');
    await row(page, 'beta').click();
    await expect(tileByLabel(page, 'beta').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await tileByLabel(page, 'beta').getByRole('button', { name: 'End session' }).click();
    await row(page, 'beta').hover();
    await page
      .locator('.sidebar-row-wrap', { hasText: 'beta' })
      .getByRole('button', { name: 'Remove from list' })
      .click();
    await expect(row(page, 'beta')).toHaveCount(0);
    await expect.poll(() => Object.values(state().sessions).some((s) => s.name === 'beta')).toBe(false);

    // A found session is resumed under its own id, in its own folder.
    await row(page, 'Parser refactor').click();
    await expect(page.locator('.tile')).toHaveCount(2);
    await expect
      .poll(() => launches().some((l) => l.event === 'start' && l.args.join(' ').includes(`--resume ${pastId}`)))
      .toBe(true);

    // Taking over the running one: confirm, it ends over there, and continues here.
    await app.evaluate(({ dialog }) => {
      Object.assign(dialog, { showMessageBox: async () => ({ response: 0, checkboxChecked: false }) });
    });
    await row(page, 'outside').click();
    await expect.poll(() => externalExited, { timeout: 15_000 }).toBe(true);
    await expect(page.locator('.tile')).toHaveCount(3, { timeout: 15_000 });
    // The name the user gave it over there comes along.
    await expect(tileByLabel(page, 'outside')).toBeVisible();
    // It never got a prompt, so there is no transcript: it starts fresh under the same id.
    await expect
      .poll(() => launches().filter((l) => l.event === 'start' && l.args.join(' ').includes(runningId)).length)
      .toBe(2);
    expect(Object.values(state().sessions).find((s) => s.claudeSessionId === runningId)).toBeTruthy();
    expect(byName('alpha').lifecycle).toBe('open');

    await quit(app);
    expect(fs.readFileSync(path.join(env.REFRAME_USER_DATA!, 'logs', 'main.log'), 'utf8')).toContain(
      'other terminal stopped',
    );
  } catch (error) {
    await captureWindows('sessions');
    throw error;
  } finally {
    if (external && !externalExited) external.kill();
    await cleanup(dir);
  }
});
