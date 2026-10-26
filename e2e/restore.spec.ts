// End to end: real app, fake claude. Creates sessions, collapses one, changes a session id with
// /clear, quits through the real shutdown path and checks that a restart restores the layout, resumes
// the used session under its latest id and starts the never-used one fresh under its own id.
import { expect, test } from '@playwright/test';
import { answerDialogs, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('sessions survive a restart with layout, dock and latest session ids', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  try {
    // --- first run ---
    let { app, page } = await launch(env);
    await expect(page.getByText('No sessions yet')).toBeVisible();

    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await expect(page.locator('.tile')).toHaveCount(2);

    // Closing the window, its sessions kept in the tray, only hides it; they keep running and the hidden state is
    // saved.
    await answerDialogs(app, 1);
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.close());
    await expect
      .poll(() => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.isVisible()))
      .toBe(false);
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
    const relaunched = () =>
      launches()
        .slice(firstRun.length)
        .filter((l) => l.event === 'start')
        .map((l) => l.args.slice(0, 2).join(' '))
        .sort();
    await expect.poll(relaunched).toEqual([`--resume ${alphaAfterClear}`, `--session-id ${betaId}`].sort());

    // Expanding beta brings it back next to alpha.
    await page.locator('.dock-item', { hasText: 'beta' }).click();
    await expect(page.locator('.tile')).toHaveCount(2);
    await expect(tileByLabel(page, 'beta').locator('.status-idle')).toBeVisible();

    await quit(app);
    const all = launches();
    expect(all.filter((l) => l.event === 'exit')).toHaveLength(all.filter((l) => l.event === 'start').length);
  } catch (error) {
    await captureWindows('restore');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
