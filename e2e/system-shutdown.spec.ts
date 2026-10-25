// End to end (Windows): before Windows shuts down, restarts or logs off, it asks every window
// (WM_QUERYENDSESSION), then closes the programs, claude among them. Sessions ended that way were not ended by
// the user: they stay open and resume on the next start. A claude that ends at any other time stays ended.
import koffi from 'koffi';
import { expect, test } from '@playwright/test';
import { captureWindows, cleanup, launch, newSession, quit, setup, tileByLabel } from './helpers';

test.skip(process.platform !== 'win32', 'Windows only');

test('sessions Windows ends on its way down resume on the next start', async () => {
  const { dir, project, env, launches, byName } = setup();
  try {
    let { app, page } = await launch(env);
    await newSession(page, project, 'alpha');
    await newSession(page, project, 'beta');
    const end = async (name: string) => {
      const start = launches().find((l) => l.event === 'start' && l.args.includes(name))!;
      process.kill(start.pid);
      // Ended (or failed to start, this soon after starting): either way the card over the terminal shows.
      await expect(tileByLabel(page, name).locator('.tile-overlay')).toBeVisible();
    };

    // alpha's claude ends while the system runs on: it stays ended.
    await end('alpha');
    expect(byName('alpha').lifecycle).toBe('ended');

    // Windows asks whether it may go down, then ends beta's claude: beta stays open.
    const hwnd = await app.evaluate(({ BrowserWindow }) =>
      BrowserWindow.getAllWindows()[0]!.getNativeWindowHandle().readBigUInt64LE().toString(),
    );
    const SendMessageW = koffi
      .load('user32.dll')
      .func('intptr_t __stdcall SendMessageW(void* hwnd, uint32_t msg, uintptr_t wParam, intptr_t lParam)');
    const WM_QUERYENDSESSION = 0x0011;
    SendMessageW(BigInt(hwnd), WM_QUERYENDSESSION, 0, 0);
    await end('beta');
    expect(byName('beta').lifecycle).toBe('open');
    await quit(app);

    ({ app, page } = await launch(env));
    await expect(tileByLabel(page, 'beta').locator('.status-idle')).toBeVisible({ timeout: 15_000 });
    await expect(tileByLabel(page, 'alpha').getByRole('button', { name: 'Resume' })).toBeVisible();
    expect(launches().filter((l) => l.event === 'start' && l.args.includes('beta'))).toHaveLength(2);
    expect(launches().filter((l) => l.event === 'start' && l.args.includes('alpha'))).toHaveLength(1);
    await quit(app);
  } catch (error) {
    await captureWindows('system-shutdown');
    throw error;
  } finally {
    await cleanup(dir);
  }
});
