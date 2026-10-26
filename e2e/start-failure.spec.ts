// End to end: a start that fails half way (here: the state cannot be written, state.json being a folder) ends the
// process with the reason in the log, instead of leaving one with no window that holds the single-instance lock.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { cleanup, runToExit, setup } from './helpers';

test('a start that fails ends the process and says why in the log', async () => {
  const { dir, env } = setup();
  try {
    const userData = env.REFRAME_USER_DATA!;
    fs.mkdirSync(path.join(userData, 'state.json'), { recursive: true });
    expect(await runToExit(env)).toBe(1);
    expect(fs.readFileSync(path.join(userData, 'logs', 'main.log'), 'utf8')).toContain('reframe could not start');
  } finally {
    await cleanup(dir);
  }
});
