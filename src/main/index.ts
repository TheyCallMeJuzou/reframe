import { app, dialog } from 'electron';
import { resolveLocale, translate } from '@shared/i18n';
import icon from '../../resources/icon.png?asset';
import trayIcon from '../../resources/tray.png?asset';
import trayIcon2x from '../../resources/tray@2x.png?asset';
import path from 'node:path';
import { ReframeApp } from './app';
import { createLogger } from './log';
import { startedBySystem } from './autostart';
import { portableDataDir } from './portable';

// Tests and side-by-side dev runs use their own profile; the portable copy keeps it next to itself.
const portable = app.isPackaged ? portableDataDir(process.execPath) : null;
const profile = process.env.REFRAME_USER_DATA || portable;
if (profile) app.setPath('userData', profile);

const build = app.isPackaged ? 'installed' : 'development';

if (!app.requestSingleInstanceLock({ build })) {
  // One reframe per profile: the running one shows its windows (second-instance) and this one leaves.
  createLogger(path.join(app.getPath('userData'), 'logs'))('info', 'reframe already runs, showing its windows', {
    build,
  });
  app.quit();
} else {
  // Windows shows toasts only for an app id with a Start menu shortcut. The installer creates one for
  // dev.reframe.app; unpackaged (npm run dev) and portable, the executable's path is the id that works.
  if (process.platform === 'win32') {
    app.setAppUserModelId(app.isPackaged && !portable ? 'dev.reframe.app' : process.execPath);
  }

  const log = createLogger(path.join(app.getPath('userData'), 'logs'));
  // Logged and survived: ending the process would end every session, and Electron's own error box is a modal
  // nobody sees while reframe sits in the tray.
  process.on('uncaughtException', (error) =>
    log('error', 'uncaught exception', { error: error.stack ?? String(error) }),
  );
  process.on('unhandledRejection', (reason) =>
    log('error', 'unhandled promise rejection', { error: (reason as Error)?.stack ?? String(reason) }),
  );

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
    try {
      reframe = new ReframeApp(locale, { icon, trayIcon, trayIcon2x }, !!portable);
      await reframe.start({ hidden: startedBySystem() });
    } catch (error) {
      // Without this, a failed start left a process with no window and no tray that still held the single-instance
      // lock: every later launch only "showed the windows" of it. Sessions started meanwhile are ended first.
      const text = (error as Error)?.stack ?? String(error);
      log('error', 'reframe could not start', { error: text });
      const ending = reframe?.shutdown().catch(() => {});
      await Promise.race([ending, new Promise((resolve) => setTimeout(resolve, 8000))]);
      if (!process.env.REFRAME_E2E) {
        dialog.showErrorBox(
          translate(locale, 'startFailed.title'),
          translate(locale, 'startFailed.message', { error: String(error), log: log.file }),
        );
      }
      app.exit(1);
      return;
    }
    // End-to-end tests drive things a real drag would do (e.g. tearing a tile out at a point).
    if (process.env.REFRAME_E2E) Object.assign(globalThis, { reframe });
  });
}
