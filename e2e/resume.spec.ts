// End to end: a session that ended by itself shows a card over its terminal, and Resume there starts it again.
// Nothing of the terminal may poke through that card: xterm's input field follows the cursor and caught the
// click where the cursor had stopped on the Resume button.
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('Resume starts a session that ended by itself again', async () => {
  const { dir, project, env, launches } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const tile = tileByLabel(page, 'alpha');

    // Claude quits (Ctrl+C twice), its cursor left out in the middle of a line.
    await tile.locator('.tile-body').click();
    await page.keyboard.press('Control+C');
    await page.keyboard.type(' '.repeat(30));
    await page.keyboard.press('Control+C');
    const resume = tile.getByRole('button', { name: 'Resume' });
    await expect(resume).toBeVisible();

    // The input field sits at the cursor, inside the tile, but under the card.
    const field = (await tile.locator('.xterm-helper-textarea').boundingBox())!;
    const body = (await tile.locator('.tile-body').boundingBox())!;
    expect(field.x).toBeGreaterThan(body.x);
    expect(field.y).toBeGreaterThan(body.y);
    const onTop = await page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest('.tile-overlay'), {
      x: field.x + field.width / 2,
      y: field.y + field.height / 2,
    });
    expect(onTop).toBe(true);

    await resume.click();
    await expect(tile.locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect(resume).toBeHidden();
    expect(launches().filter((l) => l.event === 'start')).toHaveLength(2);

    await quit(app);
  } catch (error) {
    await captureWindows('resume');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
