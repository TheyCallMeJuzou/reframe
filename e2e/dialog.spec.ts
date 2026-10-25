// End to end: the "New session" dialog names the session after its folder unless the user types a
// name, and remembers the checkbox across dialogs and restarts. Without Claude Code it says how to get it.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { captureWindows, cleanup, launch, quit, setup } from './helpers';

const open = async (page: Page) => {
  await page.getByRole('button', { name: 'New session' }).first().click();
  const dialog = page.getByRole('dialog');
  return {
    dialog,
    folder: dialog.getByPlaceholder('Path to a project folder'),
    name: dialog.getByPlaceholder('Shown on the tile and in notifications'),
    fromFolder: dialog.getByRole('checkbox', { name: 'Use the folder name' }),
  };
};

test('the session name follows the folder and the choice is remembered', async () => {
  const { dir, project, env, state } = setup();
  const other = path.join(dir, 'other-project');
  fs.mkdirSync(other);
  try {
    let { app, page } = await launch(env);
    let d = await open(page);
    await expect(d.fromFolder).toBeChecked();

    // Name follows the folder ...
    await d.folder.fill(project);
    await expect(d.name).toHaveValue('project');
    await d.folder.fill(other);
    await expect(d.name).toHaveValue('other-project');

    // ... until the user types one.
    await d.name.fill('mine');
    await d.folder.fill(project);
    await expect(d.name).toHaveValue('mine');

    // Unchecking keeps a typed name; checking again follows the folder again.
    await d.fromFolder.uncheck();
    await expect(d.name).toHaveValue('mine');
    await d.fromFolder.check();
    await expect(d.name).toHaveValue('project');

    // Turned off, it stays off in the next dialog and after a restart.
    await d.fromFolder.uncheck();
    await expect(d.name).toHaveValue('');
    await d.dialog.getByRole('button', { name: 'Cancel' }).click();
    d = await open(page);
    await expect(d.fromFolder).not.toBeChecked();
    await d.folder.fill(project);
    await expect(d.name).toHaveValue('');
    await expect.poll(() => state().preferences.nameFromFolder).toBe(false);

    await quit(app);
    ({ app, page } = await launch(env));
    d = await open(page);
    await expect(d.fromFolder).not.toBeChecked();
    await quit(app);
  } catch (error) {
    await captureWindows('dialog');
    throw error;
  } finally {
    await cleanup(dir);
  }
});

test('without Claude Code, the dialog says how to get it', async () => {
  const { dir, project, env } = setup();
  // No claude anywhere: no stand-in, and a PATH (whatever its spelling) without one.
  env.REFRAME_CLAUDE_BIN = '';
  for (const key of Object.keys(env).filter((k) => k.toUpperCase() === 'PATH')) env[key] = dir;
  try {
    const { app, page } = await launch(env);
    const d = await open(page);
    await d.folder.fill(project);
    await d.dialog.getByRole('button', { name: 'Start' }).click();
    const error = d.dialog.locator('.form-error');
    await expect(error).toContainText('Install it on this computer: claude.com/claude-code');
    if (process.platform === 'win32')
      await expect(error).toContainText('Installed it only in WSL? Choose a folder in WSL');
    else await expect(error).not.toContainText('WSL');
    await quit(app);
  } catch (error) {
    await captureWindows('dialog-no-claude');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
