import { app } from 'electron';
import { resolveLocale } from '@shared/i18n';
import icon from '../../resources/icon.png?asset';
import trayIcon from '../../resources/tray.png?asset';
import trayIcon2x from '../../resources/tray@2x.png?asset';
import { ReframeApp } from './app';

// Tests and side-by-side dev runs use their own profile.
if (process.env.REFRAME_USER_DATA) app.setPath('userData', process.env.REFRAME_USER_DATA);

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  if (process.platform === 'win32') app.setAppUserModelId('dev.reframe.app');

  let reframe: ReframeApp | null = null;
  let quitting = false;

  app.on('second-instance', () => reframe?.revealWindows());

  // Closing windows only hides them; reframe keeps running in the tray until "Quit".
  app.on('window-all-closed', () => {});

  app.on('before-quit', (event) => {
    if (quitting || !reframe) return;
    event.preventDefault();
    quitting = true;
    // Second quit() passes through (quitting is set) and lets Chromium close its helper processes.
    void reframe.shutdown().finally(() => app.quit());
  });

  app.on('activate', () => reframe?.revealWindows());

  void app.whenReady().then(async () => {
    const locale = resolveLocale(process.env.REFRAME_LOCALE || app.getLocale());
    reframe = new ReframeApp(locale, { icon, trayIcon, trayIcon2x });
    await reframe.start();
  });
}
