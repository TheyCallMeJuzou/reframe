// End to end: windows the user closed stay closed. An empty secondary window that is closed is gone for good; one
// with sessions stays in the tray and does not come back when reframe is launched a second time (only the windows
// that were open do); closed empty windows left over from older versions are dropped at the start.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type ElectronApplication } from '@playwright/test';
import { tileKey } from '../src/shared/layout';
import { callMain, captureWindows, cleanup, launch, newSession, pageOf, quit, setup, transferNode } from './helpers';

/** Which windows are visible, by window id. */
async function visible(app: ElectronApplication, ids: string[]): Promise<Record<string, boolean>> {
  const result: Record<string, boolean> = {};
  for (const id of ids) {
    const page = await pageOf(app, id);
    result[id] = await (await app.browserWindow(page)).evaluate((win) => win.isVisible());
  }
  return result;
}

const close = async (app: ElectronApplication, windowId: string) =>
  (await app.browserWindow(await pageOf(app, windowId))).evaluate((win) => win.close());

test('closed windows stay closed: empty ones go, a second launch shows only the open ones', async () => {
  const { dir, project, env, state, byName } = setup();
  try {
    const started = await launch(env);
    let app = started.app;
    const page = started.page;
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    const mainId = state().windows[0]!.id;

    // A new window closed while empty is gone, not hidden.
    let opened = app.waitForEvent('window');
    const emptyId = await callMain<string>(app, 'newWindow');
    await expect((await opened).getByText('No sessions yet')).toBeVisible();
    expect(state().windows.map((w) => w.id)).toEqual([mainId, emptyId]);
    await close(app, emptyId);
    await expect.poll(() => state().windows.map((w) => w.id)).toEqual([mainId]);
    await expect.poll(() => app.windows().length).toBe(1);

    // A window with a session in it only hides when closed.
    opened = app.waitForEvent('window');
    const secondId = await callMain<string>(app, 'newWindow');
    const second = await opened;
    await transferNode(second, tileKey(byName('beta').id), mainId, null);
    await expect(second.locator('.tile')).toHaveCount(1);
    await close(app, secondId);
    await expect.poll(() => state().windows.find((w) => w.id === secondId)?.hidden).toBe(true);

    // reframe launched again while it runs: the open window shows, the closed one stays closed.
    await callMain(app, 'revealWindows');
    expect(await visible(app, [mainId, secondId])).toEqual({ [mainId]: true, [secondId]: false });

    // All windows closed: a second launch shows the first one, as a start does.
    await close(app, mainId);
    await expect.poll(() => state().windows[0]!.hidden).toBe(true);
    await callMain(app, 'revealWindows');
    expect(await visible(app, [mainId, secondId])).toEqual({ [mainId]: true, [secondId]: false });
    await quit(app);

    // A closed empty window as older versions kept it: dropped at the start; the closed one with beta stays.
    const file = path.join(env.REFRAME_USER_DATA!, 'state.json');
    const saved = JSON.parse(fs.readFileSync(file, 'utf8'));
    const leftover = { id: 'left-over', bounds: null, maximized: false, hidden: true, layout: null, dock: [] };
    saved.windows.push(leftover);
    fs.writeFileSync(file, JSON.stringify(saved));
    ({ app } = await launch(env));
    await expect.poll(() => app.windows().length, { timeout: 10_000 }).toBe(2);
    await expect.poll(() => state().windows.map((w) => w.id)).toEqual([mainId, secondId]);
    // The window to show appears on ready-to-show, so wait for it rather than look once.
    await expect
      .poll(() => visible(app, [mainId, secondId]), { timeout: 10_000 })
      .toEqual({ [mainId]: true, [secondId]: false });
    await expect((await pageOf(app, secondId)).locator('.tile')).toHaveCount(1);
    await quit(app);
  } catch (error) {
    await captureWindows('closed-windows');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
