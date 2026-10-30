// End to end: windows the user closed stay closed. An empty secondary window that is closed is gone for good. One
// with sessions asks first: end them (the window goes; the first window stays, empty), keep them running in the
// tray, or cancel; the answer can be kept. Kept in the tray, it does not come back when reframe is launched a second
// time (only the windows that were open do); closed empty windows left over from older versions are dropped at the
// start. A restart brings no empty window back (unless all are empty): the first window with sessions is the first.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type ElectronApplication } from '@playwright/test';
import { tileKey } from '../src/shared/layout';
import {
  answerDialogs,
  callMain,
  captureWindows,
  cleanup,
  dialogsAsked,
  launch,
  newSession,
  pageOf,
  quit,
  setup,
  tileByLabel,
  transferNode,
} from './helpers';

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

    // A window with a session in it, closed keeping the session in the tray, only hides.
    opened = app.waitForEvent('window');
    const secondId = await callMain<string>(app, 'newWindow');
    const second = await opened;
    await transferNode(second, tileKey(byName('beta').id), mainId, null);
    await expect(second.locator('.tile')).toHaveCount(1);
    await answerDialogs(app, 1);
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

test('empty windows are not brought back by a restart; the window with sessions becomes the first', async () => {
  const { dir, project, env, state, byName } = setup();
  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const mainId = state().windows[0]!.id;
    // alpha moves to a second window, which leaves the first one empty; a third window is opened and left empty.
    let opened = app.waitForEvent('window');
    const secondId = await callMain<string>(app, 'newWindow');
    const second = await opened;
    await transferNode(second, tileKey(byName('alpha').id), mainId, null);
    await expect(second.locator('.tile')).toHaveCount(1);
    await expect(page.getByText('No sessions yet')).toBeVisible();
    opened = app.waitForEvent('window');
    const thirdId = await callMain<string>(app, 'newWindow');
    await expect((await opened).getByText('No sessions yet')).toBeVisible();
    expect(state().windows.map((w) => w.id)).toEqual([mainId, secondId, thirdId]);
    await quit(app);

    // Only the window with alpha comes back, as the first window now.
    ({ app, page } = await launch(env));
    await expect.poll(() => state().windows.map((w) => w.id)).toEqual([secondId]);
    await expect(tileByLabel(page, 'alpha').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    expect(app.windows()).toHaveLength(1);

    // Its session ends: the only window, empty now, comes back all the same (something has to).
    await page.locator('.tile').getByRole('button', { name: 'End session' }).click();
    await expect(page.getByText('No sessions yet')).toBeVisible();
    await quit(app);
    ({ app, page } = await launch(env));
    await expect.poll(() => state().windows.map((w) => w.id)).toEqual([secondId]);
    await expect(page.getByText('No sessions yet')).toBeVisible();
    await quit(app);
  } catch (error) {
    await captureWindows('empty-windows');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

test('closing a window with sessions asks: cancel, keep them in the tray, or end them', async () => {
  const { dir, project, env, state, byName, launches } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    const mainId = state().windows[0]!.id;
    const alpha = byName('alpha').id;
    const beta = byName('beta').id;
    const opened = app.waitForEvent('window');
    const secondId = await callMain<string>(app, 'newWindow');
    const second = await opened;
    await transferNode(second, tileKey(beta), mainId, null);
    await expect(second.locator('.tile')).toHaveCount(1);
    const exits = () => launches().filter((l) => l.event === 'exit').length;

    // Cancel: the window stays open.
    await answerDialogs(app, 2);
    await close(app, secondId);
    await expect.poll(() => dialogsAsked(app)).toEqual(['End the session in this window?']);
    expect(await visible(app, [secondId])).toEqual({ [secondId]: true });

    // Keep in the tray: hidden, beta runs on.
    await answerDialogs(app, 1);
    await close(app, secondId);
    await expect.poll(() => state().windows.find((w) => w.id === secondId)?.hidden).toBe(true);
    expect(state().sessions[beta]!.lifecycle).toBe('open');
    expect(exits()).toBe(0);

    // End sessions, and don't ask again: beta ends (it stays in the session list) and the window is gone.
    await (await app.browserWindow(second)).evaluate((win) => win.show());
    await answerDialogs(app, 0, true);
    await close(app, secondId);
    await expect.poll(() => state().windows.map((w) => w.id)).toEqual([mainId]);
    expect(state().sessions[beta]!.lifecycle).toBe('ended');
    expect(state().preferences.closeWindow).toBe('end');
    await expect.poll(exits, { timeout: 10_000 }).toBe(1);

    // From now on without asking: closing the first window ends alpha; the window stays, empty, in the tray.
    await close(app, mainId);
    await expect.poll(() => state().sessions[alpha]!.lifecycle).toBe('ended');
    await expect.poll(() => state().windows[0]!.hidden).toBe(true);
    expect(state().windows.map((w) => w.id)).toEqual([mainId]);
    await expect.poll(exits, { timeout: 10_000 }).toBe(2);
    expect(await dialogsAsked(app)).toHaveLength(3);
    await quit(app);
  } catch (error) {
    await captureWindows('closed-windows');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
