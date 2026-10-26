// End to end: the PTY host (the process that runs every claude) dies. Its sessions are gone and say so, what was
// left of their processes is ended, and a new host is there at once, so Resume and new sessions work again.
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

test('after the PTY host died, its sessions can be resumed and new ones started', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    const pids = launches().map((l) => l.pid);
    expect(pids).toHaveLength(2);

    expect(await callMain<boolean>(app, 'crashPtyHost')).toBe(true);
    for (const label of ['alpha', 'beta']) {
      await expect(tileByLabel(page, label).getByText('terminal host stopped unexpectedly')).toBeVisible();
    }
    await expect.poll(() => pids.filter(alive), { timeout: 10_000 }).toEqual([]);
    // Lost to a crash, not ended by the user: they would resume on the next start.
    expect(Object.values(state().sessions).map((s) => s.lifecycle)).toEqual(['open', 'open']);

    const alpha = tileByLabel(page, 'alpha');
    await alpha.getByRole('button', { name: 'Resume' }).click();
    await expect(alpha.locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    // Under its own id (never prompted, so started fresh under it, as on any resume).
    expect(launches().filter((l) => l.event === 'start')[2]!.args).toContain(byName('alpha').claudeSessionId);
    await newSession(page, project, 'gamma');
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(4);
    await expect(tileByLabel(page, 'beta').getByRole('button', { name: 'Resume' })).toBeVisible();

    await quit(app);
  } catch (error) {
    await captureWindows('host-crash');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
