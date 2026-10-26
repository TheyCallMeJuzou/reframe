// End to end (Windows): Claude Code's registry entry of a claude that was killed stays behind, and Windows soon gives
// its process id to another program. reframe must neither list such an entry as a session running elsewhere (taking
// it over would end that program) nor end that program as an orphan of its own; an entry whose process really is
// the one it names is listed.
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import type { ReframeApi } from '../src/shared/ipc';
import { processStart } from '../src/main/process';
import { captureWindows, cleanup, launch, quit, setup } from './helpers';

const alive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

test("registry entries left behind under another program's process id are left alone", async () => {
  test.skip(process.platform !== 'win32', 'process start times are compared on Windows');
  const { dir, project, env } = setup();
  const others: ChildProcess[] = [];
  try {
    const other = () => {
      const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
      others.push(child);
      return child.pid!;
    };
    const [orphanPid, foreignPid, realPid] = [other(), other(), other()];
    const sessions = path.join(env.REFRAME_CLAUDE_HOME!, 'sessions');
    fs.mkdirSync(sessions, { recursive: true });
    const entry = (pid: number, sessionId: string, procStart: string) =>
      fs.writeFileSync(
        path.join(sessions, `${pid}.json`),
        JSON.stringify({ pid, sessionId, cwd: project, procStart, startedAt: 1, status: 'idle' }),
      );
    entry(orphanPid, 'own-session', '1');
    entry(foreignPid, 'foreign-session', '1');
    entry(realPid, 'real-session', processStart(realPid)!);
    // A reframe session whose last process id is now another program's.
    const userData = env.REFRAME_USER_DATA!;
    fs.mkdirSync(userData, { recursive: true });
    const record = { claudeSessionId: 'own-session', cwd: project, launch: {}, lifecycle: 'ended', createdAt: 1 };
    fs.writeFileSync(
      path.join(userData, 'state.json'),
      JSON.stringify({
        version: 1,
        sessions: { a: { id: 'a', ...record, lastActiveAt: 1, pid: orphanPid } },
        windows: [{ id: 'w', bounds: null, maximized: false, hidden: false, layout: null, dock: [] }],
        recentFolders: [],
      }),
    );

    const { app, page } = await launch(env);
    const list = await page.evaluate(() =>
      (globalThis as unknown as { reframe: ReframeApi }).reframe.getSessionList('', 20),
    );
    expect(list.running.map((r) => r.claudeSessionId)).toEqual(['real-session']);
    expect([orphanPid, foreignPid, realPid].filter(alive)).toHaveLength(3);
    await quit(app);
  } catch (error) {
    await captureWindows('stale-registry');
    throw error;
  } finally {
    for (const child of others) child.kill();
    await cleanup(dir);
  }
});
