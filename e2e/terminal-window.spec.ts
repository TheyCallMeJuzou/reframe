// End to end (Windows): a terminal window with claude is dropped onto reframe. A fake claude runs under cmd in
// a real console window (or a Windows Terminal tab, if that is the default terminal). The tests do what the
// release of a dragged window, or a terminal tab torn out over reframe, does (REFRAME_E2E hooks), confirm the
// dialog, and check that the session moved into reframe and that the console, with only cmd left, was closed
// by typing "exit".
import { execFileSync, spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { callMain, captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test.skip(process.platform !== 'win32', 'terminal window takeover is Windows only for now');

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

type Setup = ReturnType<typeof setup>;

/** Starts claude (the fake) named "outside" under cmd in its own console; resolves once it is registered. */
async function outsideTerminal({ dir, project, env }: Setup, started: (shellPid: number) => void) {
  const sessionId = crypto.randomUUID();
  const noHooks = path.join(dir, 'no-hooks.json');
  fs.writeFileSync(noHooks, '{}');
  // An interactive cmd in its own console window that runs claude as a command, like typing it at the prompt.
  // (`start` gives it a console; a detached spawn would leave cmd without one, and a batch file would answer
  // Ctrl+C with "Terminate batch job?".)
  const command = `"${process.execPath}" "${env.REFRAME_CLAUDE_BIN}" --session-id ${sessionId} --name outside --settings "${noHooks}"`;
  spawn('cmd.exe', ['/c', 'start', '"reframe test terminal"', 'cmd.exe', '/s', '/k', `"${command}"`], {
    cwd: project,
    env,
    stdio: 'ignore',
    windowsVerbatimArguments: true,
  });
  const registry = path.join(env.REFRAME_CLAUDE_HOME!, 'sessions');
  const fakePid = () => {
    try {
      for (const file of fs.readdirSync(registry)) {
        const entry = JSON.parse(fs.readFileSync(path.join(registry, file), 'utf8'));
        if (entry.sessionId === sessionId) return entry.pid as number;
      }
    } catch {
      // not there yet
    }
    return 0;
  };
  await expect.poll(fakePid, { timeout: 20_000 }).toBeGreaterThan(0);
  const pid = fakePid();
  const shellPid = Number(
    execFileSync(
      'powershell',
      ['-NoProfile', '-Command', `(Get-CimInstance Win32_Process -Filter "ProcessId=${pid}").ParentProcessId`],
      { encoding: 'utf8' },
    ).trim(),
  );
  expect(shellPid).toBeGreaterThan(0);
  started(shellPid);
  return { sessionId, pid, shellPid };
}

/** Runs `run`; on failure keeps screenshots and prints the app log; always ends the outside terminal. */
async function withOutsideTerminal(name: string, run: (s: Setup, started: (pid: number) => void) => Promise<void>) {
  const s = setup();
  let shellPid = 0;
  try {
    await run(s, (pid) => (shellPid = pid));
  } catch (error) {
    await captureWindows(name);
    throw error;
  } finally {
    if (shellPid && alive(shellPid)) spawn('taskkill', ['/PID', String(shellPid), '/T', '/F'], { stdio: 'ignore' });
    await cleanup(s.dir);
  }
}

const readLog = (s: Setup) => fs.readFileSync(path.join(s.env.REFRAME_USER_DATA!, 'logs', 'main.log'), 'utf8');

test('a terminal window dropped onto reframe hands its claude over and closes', async () => {
  await withOutsideTerminal('terminal-window', async (s, started) => {
    const { sessionId, pid, shellPid } = await outsideTerminal(s, started);
    const { app, page } = await launch(s.env);
    // A group to drop into: released over its bar (not over a tile), the session must join it.
    await newSession(page, s.project, 'alpha');
    await callMain(app, 'createGroup', s.byName('alpha').id);
    const header = (await page.locator('.group-header').boundingBox())!;
    const bounds = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getContentBounds());
    const point = { x: bounds.x + header.x + header.width / 2, y: bounds.y + header.y + header.height / 2 };

    const hwnd = await callMain<string | null>(app, 'terminalWindowOf', pid);
    expect(hwnd).toBeTruthy();
    // Released twice in a row (some terminals report the end of a move twice): still one question.
    expect(await callMain<boolean>(app, 'dropTerminalWindow', hwnd, point)).toBe(true);
    expect(await callMain<boolean>(app, 'dropTerminalWindow', hwnd, point)).toBe(true);

    const dialog = page.getByRole('dialog');
    await expect(dialog).toHaveCount(1);
    await expect(dialog).toContainText('Take over from the terminal');
    await expect(dialog).toContainText('outside');
    await dialog.getByRole('button', { name: 'Take over' }).click();

    // claude left the terminal and continues in reframe, in the group, under the name it had there.
    await expect.poll(() => alive(pid), { timeout: 15_000 }).toBe(false);
    await expect(tileByLabel(page, 'outside')).toBeVisible({ timeout: 15_000 });
    await expect(page.locator('.group .tile')).toHaveCount(2);
    await expect
      .poll(() => s.launches().filter((l) => l.event === 'start' && l.args.join(' ').includes(sessionId)).length)
      .toBe(2);

    // Only cmd was left in that console, so reframe typed "exit" and the console went away.
    await expect.poll(() => alive(shellPid), { timeout: 15_000 }).toBe(false);
    expect(readLog(s)).toContain('typing exit');
    expect(readLog(s)).toContain('repeated drop of the same terminal window ignored');

    await quit(app);
  });
});

test('a terminal tab torn out over reframe (a new terminal window there) is taken over too', async () => {
  await withOutsideTerminal('terminal-tab', async (s, started) => {
    const { pid, shellPid } = await outsideTerminal(s, started);
    const { app, page } = await launch(s.env);
    await newSession(page, s.project, 'alpha');
    // Dragged over the lower part of tile alpha and released there.
    const tile = (await tileByLabel(page, 'alpha').boundingBox())!;
    const bounds = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getContentBounds());
    const client = { x: tile.x + tile.width / 2, y: tile.y + tile.height * 0.9 };
    const point = { x: bounds.x + client.x, y: bounds.y + client.y };

    const hwnd = await callMain<string | null>(app, 'terminalWindowOf', pid);
    expect(hwnd).toBeTruthy();
    // While the tab is over reframe, the window sees a drag without data types and shows the drop zone.
    await callMain(app, 'tabDragSource', hwnd);
    const dragEvent = (type: string) =>
      page.evaluate(
        ({ type, client, point }) => {
          const init = { bubbles: true, dataTransfer: new DataTransfer(), relatedTarget: null };
          const at = { clientX: client.x, clientY: client.y, screenX: point.x, screenY: point.y };
          const target = document.elementFromPoint(client.x, client.y) ?? document.body;
          target.dispatchEvent(new DragEvent(type, { ...init, ...at }));
        },
        { type, client, point },
      );
    await dragEvent('dragover');
    const preview = page.locator('.foreign-preview');
    await expect(preview).toBeVisible();
    const zone = (await preview.boundingBox())!;
    expect(zone.y).toBeGreaterThan(tile.y + tile.height / 3); // the lower half of alpha
    // Released: the drag leaves the window (nobody accepts it), and the terminal opens a window for the tab.
    await dragEvent('dragleave');
    await expect(preview).toBeHidden();
    await callMain(app, 'terminalWindowAppeared', hwnd);

    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('outside', { timeout: 10_000 });
    await dialog.getByRole('button', { name: 'Take over' }).click();

    await expect.poll(() => alive(pid), { timeout: 15_000 }).toBe(false);
    await expect(tileByLabel(page, 'outside')).toBeVisible({ timeout: 15_000 });
    // Below alpha: the two tiles are stacked.
    const alpha = (await tileByLabel(page, 'alpha').boundingBox())!;
    const outside = (await tileByLabel(page, 'outside').boundingBox())!;
    expect(outside.y).toBeGreaterThan(alpha.y + alpha.height / 2);
    await expect.poll(() => alive(shellPid), { timeout: 15_000 }).toBe(false);
    expect(readLog(s)).toContain('"pointFrom":"drag over reframe"');

    await quit(app);
  });
});
