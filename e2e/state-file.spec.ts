// End to end: a state file reframe cannot use is never written over. One from a newer reframe makes this one quit
// and leave it as it is (going back to that version keeps everything); any other is set aside and the backup used.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, quit, runToExit, setup } from './helpers';

test('a state file from a newer reframe is left alone, and reframe quits', async () => {
  const { dir, env } = setup();
  try {
    const userData = env.REFRAME_USER_DATA!;
    fs.mkdirSync(userData, { recursive: true });
    const newer = JSON.stringify({ version: 2, sessions: { a: { from: 'the future' } }, windows: [] });
    fs.writeFileSync(path.join(userData, 'state.json'), newer);

    expect(await runToExit(env)).toBe(1);
    expect(fs.readFileSync(path.join(userData, 'state.json'), 'utf8')).toBe(newer);
    expect(fs.existsSync(path.join(userData, 'state.json.bak'))).toBe(false);
    expect(fs.readFileSync(path.join(userData, 'logs', 'main.log'), 'utf8')).toContain('from a newer reframe');
  } finally {
    await cleanup(dir);
  }
});

test('an unusable state file is set aside and the backup takes over', async () => {
  const { dir, project, env, state } = setup();
  try {
    const userData = env.REFRAME_USER_DATA!;
    fs.mkdirSync(userData, { recursive: true });
    fs.writeFileSync(path.join(userData, 'state.json'), '{ "version": 1, "sessions": ');
    const backup = { version: 1, sessions: {}, windows: [], recentFolders: [project] };
    fs.writeFileSync(path.join(userData, 'state.json.bak'), JSON.stringify(backup));

    const { app } = await launch(env);
    expect(state().recentFolders).toEqual([project]);
    const aside = fs.readdirSync(userData).filter((f) => f.startsWith('state.json.rejected-'));
    expect(aside).toHaveLength(1);
    expect(fs.readFileSync(path.join(userData, aside[0]!), 'utf8')).toBe('{ "version": 1, "sessions": ');
    expect(fs.readFileSync(path.join(userData, 'logs', 'main.log'), 'utf8')).toContain('could not be used, set aside');
    await quit(app);
  } catch (error) {
    await captureWindows('state-file');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
