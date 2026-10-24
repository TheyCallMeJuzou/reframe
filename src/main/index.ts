import { app } from 'electron';
import { resolveLocale } from '@shared/i18n';
import icon from '../../resources/icon.png?asset';
import trayIcon from '../../resources/tray.png?asset';
import trayIcon2x from '../../resources/tray@2x.png?asset';
import path from 'node:path';
import { ReframeApp } from './app';
import { createLogger } from './log';
import { startedBySystem } from './autostart';

// Tests and side-by-side dev runs use their own profile.
if (process.env.REFRAME_USER_DATA) app.setPath('userData', process.env.REFRAME_USER_DATA);

const build = app.isPackaged ? 'installed' : 'development';

if (!app.requestSingleInstanceLock({ build })) {
  // One reframe per profile: the running one shows its windows (second-instance) and this one leaves.
  createLogger(path.join(app.getPath('userData'), 'logs'))('info', 'reframe already runs, showing its windows', {
    build,
  });
  app.quit();
} else {
  // Windows shows toasts only for an app id with a Start menu shortcut. The installer creates one for
  // dev.reframe.app; unpackaged (npm run dev) Electron's own executable path is the id that works.
  if (process.platform === 'win32') app.setAppUserModelId(app.isPackaged ? 'dev.reframe.app' : process.execPath);

  let reframe: ReframeApp | null = null;
  let quitting = false;

  app.on('second-instance', (_event, _argv, _cwd, data) => {
    reframe?.revealWindows();
    // The installed app started while a development build runs, or the other way round: say so, or one ends up
    // trying out the build that is not running.
    const other = (data as { build?: unknown } | null)?.build;
    if ((other === 'installed' || other === 'development') && other !== build) reframe?.otherBuildStarted(other);
  });

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
    await reframe.start({ hidden: startedBySystem() });
    // End-to-end tests drive things a real drag would do (e.g. tearing a tile out at a point).
    if (process.env.REFRAME_E2E) Object.assign(globalThis, { reframe });
  });
}
