// End to end: files dropped onto a tile are typed in as their paths (one that exists only in memory is saved to
// a temporary file first), and Ctrl+V pastes text, the paths of copied files, or leaves an image to Claude Code
// (its paste key, which the fake answers with an "[Image #n]" chip). Right-click copies the selection or, without
// one, pastes, unless the program reads the mouse (then Shift+right-click does). The clipboard is faked through a
// test hook, so the real one is only read, never changed.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test('dropping files and pasting text, files and images', async () => {
  const { dir, project, env, byName } = setup();
  try {
    const { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    const claudeSession = byName('alpha').claudeSessionId;
    const transcript = path.join(env.REFRAME_CLAUDE_HOME!, 'projects', 'fake-project', `${claudeSession}.jsonl`);
    const prompts = () =>
      fs.existsSync(transcript)
        ? fs
            .readFileSync(transcript, 'utf8')
            .trim()
            .split('\n')
            .map((line) => (JSON.parse(line) as { prompt: string }).prompt)
        : [];
    const send = async () => {
      await page.keyboard.press('Enter');
    };

    // The real clipboard can be read (but stays untouched).
    const real = await page.evaluate(() => window.reframe.readClipboard());
    expect(typeof real.text).toBe('string');
    expect(Array.isArray(real.files)).toBe(true);

    // A file dragged in from memory (e.g. an image from a browser): a hint while over the tile, its path on release.
    const body = tileByLabel(page, 'alpha').locator('.tile-body');
    await body.evaluate((el) => {
      const data = new DataTransfer();
      data.items.add(new File(['hello'], 'note one.txt', { type: 'text/plain' }));
      (window as unknown as { dropData: DataTransfer }).dropData = data;
      el.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: data }));
    });
    await expect(page.locator('.files-drop')).toBeVisible();
    await body.evaluate((el) => {
      const data = (window as unknown as { dropData: DataTransfer }).dropData;
      el.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: data }));
    });
    await expect(page.locator('.files-drop')).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => document.activeElement?.closest('.tile') !== null)).toBe(true);
    await page.waitForTimeout(300);
    await send();
    await expect.poll(prompts).toHaveLength(1);
    const dropped = prompts()[0]!
      .trim()
      .replace(/^"|"$|^'|'$/g, '');
    expect(path.basename(dropped)).toBe('note one.txt');
    expect(fs.readFileSync(dropped, 'utf8')).toBe('hello');

    // Ctrl+V: text first, then copied files (quoted as needed), then an image for Claude Code to fetch.
    const copied = process.platform === 'win32' ? 'C:\\My Pictures\\shot.png' : '/tmp/my shot.png';
    const quoted = process.platform === 'win32' ? `"${copied}"` : `'${copied}'`;
    const paste = async (contents: { text: string; image: boolean; files: string[] }) => {
      await callMain(app, 'fakeClipboard', contents);
      await page.keyboard.press('Control+V');
      await page.waitForTimeout(300);
      await send();
    };
    await paste({ text: 'pasted words', image: true, files: [] });
    await paste({ text: '', image: false, files: [copied] });
    await paste({ text: '', image: true, files: [] });
    await expect.poll(prompts).toHaveLength(4);
    expect(
      prompts()
        .slice(1)
        .map((p) => p.trim()),
    ).toEqual(['pasted words', quoted, '[Image #1]']);

    // Right-click copies the selection (the copy is caught, the real clipboard stays as it is) and clears it, so the
    // next right-click pastes.
    await page.evaluate(() => {
      Object.defineProperty(navigator.clipboard, 'writeText', {
        value: async (text: string) => void ((window as unknown as { copied: string }).copied = text),
      });
    });
    const screen = await tileByLabel(page, 'alpha').locator('.xterm-screen').boundingBox();
    // "FAKE CLAUDE" on the first line: a double click selects its first word.
    const word = { x: screen!.x + 10, y: screen!.y + 8 };
    await page.mouse.dblclick(word.x, word.y);
    await page.mouse.click(word.x, word.y, { button: 'right' });
    await expect.poll(() => page.evaluate(() => (window as unknown as { copied?: string }).copied)).toBe('FAKE');
    await callMain(app, 'fakeClipboard', { text: 'right-clicked words', image: false, files: [] });
    await page.mouse.click(word.x, word.y, { button: 'right' });
    await page.waitForTimeout(300);
    await send();
    await expect.poll(prompts).toHaveLength(5);
    expect(prompts()[4]!.trim()).toBe('right-clicked words');

    // A program that reads the mouse (Claude Code's fullscreen mode) gets the right-click and pastes on its own:
    // reframe stays out of it, or the paste would come twice. Shift+right-click is reframe's again.
    await page.keyboard.type('/mouse');
    await send();
    await callMain(app, 'fakeClipboard', { text: 'shift words', image: false, files: [] });
    await page.waitForTimeout(300);
    await page.mouse.click(word.x, word.y, { button: 'right' });
    await page.waitForTimeout(300);
    await send();
    await expect.poll(prompts).toHaveLength(6);
    expect(prompts()[5]!.trim()).toBe('[right click]');
    await page.keyboard.down('Shift');
    await page.mouse.click(word.x, word.y, { button: 'right' });
    await page.keyboard.up('Shift');
    await page.waitForTimeout(300);
    await send();
    await expect.poll(prompts).toHaveLength(7);
    expect(prompts()[6]!.trim()).toBe('shift words');

    await callMain(app, 'fakeClipboard', null);
    await quit(app);
  } catch (error) {
    await captureWindows('files');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
