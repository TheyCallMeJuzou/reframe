// End to end: several windows. Tiles are torn out into new windows and moved between windows without
// restarting their sessions; empty secondary windows close; hidden windows and their docks come back.
// A real OS drag between windows cannot be scripted, so the test calls what a drop calls: the renderer's
// transferNode and main's tear-out at a screen point (REFRAME_E2E hook).
import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { tileKey } from '../src/shared/layout';
import {
  captureWindows,
  cleanup,
  launch,
  newSession,
  pageOf,
  quit,
  setup,
  tearOutNode,
  tileByLabel,
  transferNode,
} from './helpers';

const tearOut = (app: ElectronApplication, sessionId: string, point: { x: number; y: number }) =>
  tearOutNode(app, tileKey(sessionId), point);

const transfer = (
  page: Page,
  sessionId: string,
  fromWindowId: string,
  target: { key: string; side: 'left' | 'right' | 'top' | 'bottom' } | null,
) => transferNode(page, tileKey(sessionId), fromWindowId, target);

test('tiles move between windows, empty windows close, all windows come back', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  try {
    let { app, page } = await launch(env);
    const area = await app.evaluate(({ screen }) => screen.getPrimaryDisplay().workArea);
    await app.evaluate(({ BrowserWindow }, b) => BrowserWindow.getAllWindows()[0]!.setBounds(b), {
      x: area.x + 20,
      y: area.y + 20,
      width: 700,
      height: 520,
    });
    const outside = { x: area.x + area.width - 420, y: area.y + 80 };
    const inside = { x: area.x + 300, y: area.y + 200 };

    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    await newSession(page, project, 'gamma');
    const alpha = byName('alpha').id;
    const beta = byName('beta').id;
    const gamma = byName('gamma').id;
    const mainId = state().windows[0]!.id;

    // Letting go over one of our windows is not a tear-out.
    expect(await tearOut(app, alpha, inside)).toBe(false);

    // Tear beta out: a new window at the pointer, same process (no new claude start).
    let opened = app.waitForEvent('window');
    expect(await tearOut(app, beta, outside)).toBe(true);
    const second = await opened;
    await expect(tileByLabel(second, 'beta').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect(page.locator('.tile')).toHaveCount(2);
    expect(state().windows).toHaveLength(2);
    const secondId = state().windows[1]!.id;
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(3);

    // Move beta back under alpha: the second window is empty now and closes.
    const secondClosed = second.waitForEvent('close');
    await transfer(page, beta, secondId, { key: `tile:${alpha}`, side: 'bottom' });
    await secondClosed;
    await expect(page.locator('.tile')).toHaveCount(3);
    expect(state().windows.map((w) => w.id)).toEqual([mainId]);

    // Tear gamma out and collapse it there: a window with only a collapsed tile stays. Then hide it (X).
    opened = app.waitForEvent('window');
    expect(await tearOut(app, gamma, outside)).toBe(true);
    const third = await opened;
    await tileByLabel(third, 'gamma').getByRole('button', { name: 'Collapse to the side bar' }).click();
    await expect(third.locator('.dock-item', { hasText: 'gamma' })).toBeVisible();
    const thirdId = state().windows[1]!.id;
    await (await app.browserWindow(third)).evaluate((win) => win.close());
    await expect.poll(() => state().windows[1]!.hidden, { timeout: 5_000 }).toBe(true);
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(3);

    await quit(app);

    // --- restart: both windows come back, the second one still hidden with gamma collapsed ---
    ({ app, page } = await launch(env));
    await expect.poll(() => app.windows().length, { timeout: 10_000 }).toBe(2);
    // The visible one appears on ready-to-show, so wait for it rather than look once.
    const visibility = () =>
      app.evaluate(({ BrowserWindow }) =>
        BrowserWindow.getAllWindows()
          .map((w) => w.isVisible())
          .sort(),
      );
    await expect.poll(visibility, { timeout: 10_000 }).toEqual([false, true]);
    const mainPage = await pageOf(app, mainId);
    const hiddenPage = await pageOf(app, thirdId);
    await expect(mainPage.locator('.tile')).toHaveCount(2);
    await expect(hiddenPage.locator('.dock-item', { hasText: 'gamma' })).toHaveCount(1);

    // Empty the main window into the other one: the first window always stays, showing its empty state.
    await transfer(hiddenPage, alpha, mainId, null);
    await transfer(hiddenPage, beta, mainId, { key: `tile:${alpha}`, side: 'right' });
    await expect(mainPage.getByText('No sessions yet')).toBeVisible();
    await expect(hiddenPage.locator('.tile')).toHaveCount(2);
    expect(state().windows).toHaveLength(2);

    // An empty window takes a dropped tile as a whole.
    await transfer(mainPage, alpha, thirdId, null);
    await expect(mainPage.locator('.tile')).toHaveCount(1);
    await expect(tileByLabel(mainPage, 'alpha').locator('.status-idle')).toBeVisible();

    await quit(app);
    const all = launches();
    expect(all.filter((l) => l.event === 'exit')).toHaveLength(all.filter((l) => l.event === 'start').length);
  } catch (error) {
    await captureWindows('windows');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
