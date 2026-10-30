// End to end: a window saved as a workspace (from the toolbar's Workspaces menu, with a name) opens again later as
// a window of its own, with its group and sessions resumed; opened while its window is there, that one just comes
// up. Workspaces survive a restart, and are renamed and deleted from the session list.
import { expect, test, type Page } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, pageOf, quit, setup } from './helpers';

test('workspaces: save a window, open it again, rename and delete it', async () => {
  const { dir, project, env, launches, state, byName } = setup();
  const starts = (id: string) =>
    launches().filter((l) => l.event === 'start' && l.args.includes(byName(id).claudeSessionId)).length;
  try {
    let { app, page } = await launch(env);

    // A second window with two sessions in a group, saved as "Frontend".
    let opened = app.waitForEvent('window');
    await page.keyboard.press('ControlOrMeta+Shift+N');
    const second = await opened;
    await second.waitForSelector('.app');
    await newSession(second, project, 'alpha');
    await newSession(second, project, 'beta');
    await callMain(app, 'createGroup', byName('alpha').id);
    const groupId = (await second.locator('.group').getAttribute('data-group'))!;
    await callMain(app, 'addSessionToGroup', byName('beta').id, groupId);
    await expect(second.locator('.group .tile')).toHaveCount(2);

    await second.getByRole('button', { name: 'Workspaces' }).click();
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Save this window as a workspace …')).toBe(true);
    const naming = second.getByRole('dialog', { name: 'Save as a workspace' });
    await naming.getByRole('textbox').fill('Frontend');
    await naming.getByRole('button', { name: 'Save' }).click();
    await expect.poll(() => state().workspaces.map((w) => w.name)).toEqual(['Frontend']);
    await expect(second.locator('.toolbar')).toContainText('Frontend');

    // Its sessions end, and the emptied window closes.
    await callMain(app, 'endGroup', groupId);
    await expect.poll(() => app.windows().length).toBe(1);

    // Opened from the session list: a window of its own again, with the group and both sessions resumed.
    await page.getByRole('button', { name: 'Sessions', exact: true }).click();
    const row = page.locator('.workspace-row', { hasText: 'Frontend' });
    opened = app.waitForEvent('window');
    await row.click();
    const restored = await opened;
    await expect(restored.locator('.group .tile')).toHaveCount(2);
    await expect.poll(() => [starts('alpha'), starts('beta')]).toEqual([2, 2]);
    await expect(row).toHaveClass(/is-open/);

    // Again while its window is open: that one comes up, no second copy.
    await row.click();
    await page.waitForTimeout(500);
    expect(app.windows()).toHaveLength(2);

    // After a restart: still saved, and its window is back like any other. The first window, never used, is not
    // (empty windows are not brought back), so the workspace's window is the first window now.
    await quit(app);
    ({ app, page } = await launch(env));
    const workspaceWindow = state().windows.find((w) => w.workspaceId)!;
    await expect.poll(() => state().windows.map((w) => w.id)).toEqual([workspaceWindow.id]);
    const other: Page = await pageOf(app, workspaceWindow.id);
    await expect(other.locator('.toolbar')).toContainText('Frontend');

    // Rename and delete from the list, opened in that window.
    await other.getByRole('button', { name: 'Sessions', exact: true }).click();
    await expect(other.locator('.sidebar')).toBeVisible();
    const listed = other.locator('.workspace-row', { hasText: 'Frontend' });
    await listed.click({ button: 'right' });
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Rename')).toBe(true);
    await other.locator('.sidebar-name-input').fill('Backend');
    await other.keyboard.press('Enter');
    await expect(other.locator('.workspace-row', { hasText: 'Backend' })).toBeVisible();
    await expect(other.locator('.toolbar')).toContainText('Backend');

    await app.evaluate(({ dialog }) => {
      Object.assign(dialog, { showMessageBox: async () => ({ response: 0, checkboxChecked: false }) });
    });
    await other.locator('.workspace-row', { hasText: 'Backend' }).click({ button: 'right' });
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Delete')).toBe(true);
    await expect(other.locator('.workspace-row')).toHaveCount(0);
    await expect.poll(() => state().workspaces).toEqual([]);
    await expect(other.locator('.toolbar')).not.toContainText('Backend');
    await expect(other.locator('.group .tile')).toHaveCount(2); // the window itself stays
    await quit(app);
  } catch (error) {
    await captureWindows('workspaces');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

test('a workspace saved from the main window opens as a window of its own once that window became another', async () => {
  const { dir, project, env, launches, byName } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await page.getByRole('button', { name: 'Workspaces' }).click();
    expect(await callMain<boolean>(app, 'clickMenuItem', 'Save this window as a workspace …')).toBe(true);
    const naming = page.getByRole('dialog', { name: 'Save as a workspace' });
    await naming.getByRole('textbox').fill('Main work');
    await naming.getByRole('button', { name: 'Save' }).click();
    await expect(page.locator('.toolbar')).toContainText('Main work');

    // Its session ends: the main window stays (it always does), but it is not that workspace any more.
    await page.locator('.tile').getByRole('button', { name: 'End session' }).click();
    await expect(page.locator('.tile')).toHaveCount(0);
    await expect(page.locator('.toolbar')).not.toContainText('Main work');

    // So opening the workspace gives it a window of its own again, with the session resumed.
    await page.getByRole('button', { name: 'Sessions', exact: true }).click();
    const row = page.locator('.workspace-row', { hasText: 'Main work' });
    await expect(row).not.toHaveClass(/is-open/);
    const opened = app.waitForEvent('window');
    await row.click();
    const restored = await opened;
    await expect(restored.locator('.tile')).toHaveCount(1);
    await expect(restored.locator('.toolbar')).toContainText('Main work');
    const alpha = byName('alpha').claudeSessionId;
    await expect.poll(() => launches().filter((l) => l.event === 'start' && l.args.includes(alpha)).length).toBe(2);
    await quit(app);
  } catch (error) {
    await captureWindows('workspace-main-window');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
