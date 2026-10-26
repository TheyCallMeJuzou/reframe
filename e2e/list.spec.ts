// End to end: selecting several entries in the session list (Ctrl/Shift+click) and opening them together from the
// right-click menu, in a new group here or in a new window (sessions shown already move along), and entries that
// pop up while reframe runs: highlighted in the list, and a dot on the Sessions button while the list is hidden.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('session list: select several, open them together, see new ones pop up', async () => {
  const { dir, project, env, launches, state } = setup();
  // Past sessions, as Claude Code leaves them: a transcript (in the fake's folder, so they resume for real).
  const transcripts = path.join(env.REFRAME_CLAUDE_HOME!, 'projects', 'fake-project');
  fs.mkdirSync(transcripts, { recursive: true });
  const past = (title: string, minutesAgo: number) => {
    const id = crypto.randomUUID();
    const file = path.join(transcripts, `${id}.jsonl`);
    const lines = [
      { type: 'user', cwd: project, message: { content: 'some work' } },
      { type: 'ai-title', aiTitle: title },
    ];
    fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
    const when = new Date(Date.now() - minutesAgo * 60_000);
    fs.utimesSync(file, when, when);
    return id;
  };
  const alpha = past('Alpha task', 10);
  past('Beta task', 20);
  const gamma = past('Gamma task', 30);
  const starts = (claudeSession: string) =>
    launches().filter((l) => l.event === 'start' && l.args.includes(claudeSession)).length;
  const recordOf = (claudeSession: string) =>
    Object.values(state().sessions).find((s) => s.claudeSessionId === claudeSession)!.id;

  try {
    const { app, page } = await launch(env);
    const sessionsButton = page.getByRole('button', { name: 'Sessions', exact: true });
    await sessionsButton.click();
    const row = (title: string) => page.locator('.sidebar-row', { hasText: title });
    await expect(row('Gamma task')).toBeVisible();

    // Ctrl+click and Shift+click select without opening anything.
    await row('Alpha task').click({ modifiers: ['ControlOrMeta'] });
    await row('Gamma task').click({ modifiers: ['Shift'] });
    await expect(page.locator('.sidebar-row.is-selected')).toHaveCount(3);
    await row('Beta task').click({ modifiers: ['ControlOrMeta'] });
    await expect(page.locator('.sidebar-row.is-selected')).toHaveCount(2);
    await expect(page.locator('.tile')).toHaveCount(0);

    // Right-click → "Open in a new group": both resume, framed together.
    await row('Alpha task').click({ button: 'right' });
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Open in a new group')).toBe(true);
    await expect(page.locator('.group .tile')).toHaveCount(2);
    await expect.poll(() => [starts(alpha), starts(gamma)]).toEqual([1, 1]);

    // Beta, plus Alpha that is open already, "in a new group in a new window": Alpha moves, without a restart.
    await row('Beta task').click({ modifiers: ['ControlOrMeta'] });
    await page.locator(`.sidebar-row[data-session="${recordOf(alpha)}"]`).click({ modifiers: ['ControlOrMeta'] });
    await expect(page.locator('.sidebar-row.is-selected')).toHaveCount(2);
    const opened = app.waitForEvent('window');
    await row('Beta task').click({ button: 'right' });
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Open in a new group in a new window')).toBe(true);
    const second = await opened;
    await expect(second.locator('.group .tile')).toHaveCount(2);
    await expect(page.locator('.tile')).toHaveCount(1);
    expect(starts(alpha)).toBe(1);

    // A session used just now pops up in the open list, highlighted for a moment.
    past('Delta task', 0);
    await expect(row('Delta task')).toHaveClass(/is-new/, { timeout: 10_000 });
    await expect(row('Delta task')).not.toHaveClass(/is-new/, { timeout: 10_000 });

    // While the list is hidden, the Sessions button shows a dot until the list is shown again.
    await sessionsButton.click();
    past('Epsilon task', 0);
    await expect(page.locator('.toolbar-dot')).toBeVisible({ timeout: 15_000 });
    await sessionsButton.click();
    await expect(page.locator('.toolbar-dot')).toHaveCount(0);
    await expect(row('Epsilon task')).toHaveClass(/is-new/);

    await quit(app);
  } catch (error) {
    await captureWindows('list');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

test('a collapsed session dragged out of the list lands where it is dropped', async () => {
  const { dir, project, env, state } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await tileByLabel(page, 'beta').getByRole('button', { name: 'Collapse to the side bar' }).click();
    await expect(page.locator('.dock-item', { hasText: 'beta' })).toBeVisible();
    await page.getByRole('button', { name: 'Sessions', exact: true }).click();

    const target = tileByLabel(page, 'alpha').locator('.tile-body');
    const box = (await target.boundingBox())!;
    await page
      .locator('.sidebar-row', { hasText: 'beta' })
      .dragTo(target, { targetPosition: { x: box.width / 2, y: box.height * 0.9 } });
    await expect(page.locator('.tile')).toHaveCount(2);
    await expect(page.locator('.dock-item')).toHaveCount(0);
    // Below alpha, where it was dropped.
    const alpha = (await tileByLabel(page, 'alpha').boundingBox())!;
    const beta = (await tileByLabel(page, 'beta').boundingBox())!;
    expect(beta.y).toBeGreaterThan(alpha.y + alpha.height / 2);
    await expect.poll(() => state().windows[0]!.dock).toEqual([]);
    await quit(app);
  } catch (error) {
    await captureWindows('list-collapsed');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
