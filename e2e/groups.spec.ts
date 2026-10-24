// End to end: groups. Created and changed through the actions behind the native menus (which Playwright
// cannot click), grown by a real in-window drag and drop, collapsed, restored after a restart, torn out
// into a window of their own, moved back, dissolved, and ended as a whole.
import { expect, test, type Locator } from '@playwright/test';
import { GROUP_COLORS } from '../src/shared/groups';
import { tileKey } from '../src/shared/layout';
import {
  callMain,
  captureWindows,
  cleanup,
  launch,
  newSession,
  quit,
  setup,
  tearOutNode,
  tileByLabel,
  transferNode,
} from './helpers';

/**
 * Drags a tile's header onto the right part of another tile. Not onto its very edge: inside a group that
 * strip belongs to the group frame and means "next to the group".
 */
async function dragNextTo(source: Locator, target: Locator): Promise<void> {
  const box = (await target.locator('.tile-body').boundingBox())!;
  await source.locator('.tile-header').dragTo(target.locator('.tile-body'), {
    targetPosition: { x: box.width * 0.7, y: box.height / 2 },
  });
}

test('groups: create, grow by drag, rename, recolor, collapse, restart, tear out, dissolve, end all', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  try {
    let { app, page } = await launch(env);
    const area = await app.evaluate(({ screen }) => screen.getPrimaryDisplay().workArea);
    await app.evaluate(({ BrowserWindow }, b) => BrowserWindow.getAllWindows()[0]!.setBounds(b), {
      x: area.x + 10,
      y: area.y + 10,
      width: Math.min(1100, area.width - 460),
      height: 700,
    });
    const outside = { x: area.x + area.width - 300, y: area.y + 80 };

    for (const name of ['alpha', 'beta', 'gamma', 'delta']) await newSession(page, project, name);
    const [alpha, gamma, delta] = ['alpha', 'gamma', 'delta'].map((n) => byName(n).id) as [string, string, string];
    // Always the current page's frame (the page changes with the restart).
    const frame = () => page.locator('.group');

    // "New group" frames alpha with an automatic name and color.
    await callMain(app, 'createGroup', alpha);
    await expect(frame().locator('.group-name')).toHaveText('Group 1');
    const groupId = (await frame().getAttribute('data-group'))!;

    // Dragging beta onto alpha (inside the frame) makes it join the group.
    await dragNextTo(tileByLabel(page, 'beta'), tileByLabel(page, 'alpha'));
    await expect(frame().locator('.group-count')).toHaveText('2');
    await expect(frame().locator('.tile')).toHaveCount(2);

    // Dropping it on the frame's edge takes it out again, next to the group; then back in.
    const box = (await frame().boundingBox())!;
    await tileByLabel(page, 'beta')
      .locator('.tile-header')
      .dragTo(frame(), { targetPosition: { x: box.width - 4, y: box.height / 2 } });
    await expect(frame().locator('.group-count')).toHaveText('1');
    await expect(page.locator('.tile')).toHaveCount(4);
    await dragNextTo(tileByLabel(page, 'beta'), tileByLabel(page, 'alpha'));
    await expect(frame().locator('.group-count')).toHaveText('2');

    // "Add to group" and "Remove from group".
    await callMain(app, 'addSessionToGroup', gamma, groupId);
    await expect(frame().locator('.group-count')).toHaveText('3');
    await callMain(app, 'removeSessionFromGroup', gamma);
    await expect(frame().locator('.group-count')).toHaveText('2');

    // Rename in place, recolor from the palette.
    await frame().locator('.group-name').dblclick();
    await frame().locator('.group-name-input').fill('API');
    await frame().locator('.group-name-input').press('Enter');
    await expect(frame().locator('.group-name')).toHaveText('API');
    await callMain(app, 'recolorGroup', groupId, GROUP_COLORS[1].hex);
    await expect.poll(() => JSON.stringify(state().windows[0]!.layout)).toContain(GROUP_COLORS[1].hex);

    // Collapsing the group puts it into the side bar as one entry.
    await frame().getByRole('button', { name: 'Collapse the group to the side bar' }).click();
    await expect(page.locator('.tile')).toHaveCount(2);
    await expect(page.locator('.dock-group')).toContainText('API');

    await quit(app);

    // --- restart: still collapsed, with its name and color ---
    ({ app, page } = await launch(env));
    await expect(page.locator('.dock-group')).toContainText('API');
    await page.locator('.dock-group').click();
    await expect(frame().locator('.tile')).toHaveCount(2);
    await expect(frame()).toHaveCSS('border-color', 'rgb(91, 156, 240)');

    // The whole group gets its own window, and comes back next to delta.
    const opened = app.waitForEvent('window');
    expect(await tearOutNode(app, groupId, outside)).toBe(true);
    const second = await opened;
    await expect(second.locator('.group .tile')).toHaveCount(2);
    await expect(page.locator('.tile')).toHaveCount(2);
    const secondId = state().windows[1]!.id;
    const secondClosed = second.waitForEvent('close');
    await transferNode(page, groupId, secondId, { key: tileKey(delta), side: 'bottom' });
    await secondClosed;
    await expect(frame().locator('.tile')).toHaveCount(2);
    await expect(page.locator('.tile')).toHaveCount(4);

    // Dissolving keeps the tiles; a fresh group of gamma and delta is ended as a whole.
    await callMain(app, 'dissolveGroup', groupId);
    await expect(page.locator('.group')).toHaveCount(0);
    await expect(page.locator('.tile')).toHaveCount(4);
    await callMain(app, 'createGroup', gamma);
    const nextGroupId = (await frame().getAttribute('data-group'))!;
    await expect(frame().locator('.group-name')).toHaveText('Group 1');
    await callMain(app, 'addSessionToGroup', delta, nextGroupId);
    await callMain(app, 'endGroup', nextGroupId);
    await expect(page.locator('.tile')).toHaveCount(2);
    await expect.poll(() => [byName('gamma').lifecycle, byName('delta').lifecycle]).toEqual(['ended', 'ended']);

    await quit(app);
    const all = launches();
    expect(all.filter((l) => l.event === 'exit')).toHaveLength(all.filter((l) => l.event === 'start').length);
    expect(byName('beta').lifecycle).toBe('open');
  } catch (error) {
    await captureWindows('groups');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
