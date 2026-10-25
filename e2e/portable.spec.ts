import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { captureWindows, launch, quit, setup } from './helpers';

// The portable copy is the packaged app with a folder "data" next to reframe.exe: only against a packaged build
// (REFRAME_E2E_APP), on Windows.
const packaged = process.env.REFRAME_E2E_APP;
test.skip(!packaged || process.platform !== 'win32', 'needs the packaged Windows app (REFRAME_E2E_APP)');

test('the portable copy keeps its data next to reframe.exe', async () => {
  const data = path.join(path.dirname(packaged!), 'data');
  test.skip(fs.existsSync(data), `${data} is there already`);
  const { env } = setup();
  delete env.REFRAME_USER_DATA;
  fs.mkdirSync(data);
  try {
    const { app } = await launch(env);
    try {
      expect(await app.evaluate(({ app: electronApp }) => electronApp.getPath('userData'))).toBe(data);
      await quit(app);
    } catch (error) {
      await captureWindows('portable');
      throw error;
    }
    expect(fs.existsSync(path.join(data, 'state.json'))).toBe(true);
    expect(fs.readFileSync(path.join(data, 'logs', 'main.log'), 'utf8')).toContain('"portable"');
  } finally {
    fs.rmSync(data, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});
