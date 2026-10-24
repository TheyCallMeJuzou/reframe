// Updates from GitHub Releases (electron-updater), for installed builds only. Checks a while after the start
// and then every few hours, downloads in the background, and installs on the next quit, or right away on
// "Restart to update" (sessions end the usual way first and resume after the restart).
import { EventEmitter } from 'node:events';
import { app } from 'electron';
import electronUpdater from 'electron-updater';
import type { LogFn } from './log';

const FIRST_CHECK_MS = 30_000;
const CHECK_EVERY_MS = 6 * 60 * 60_000;

type Events = { ready: [version: string] };

export class Updates extends EventEmitter<Events> {
  /** Version downloaded and waiting to be installed. */
  ready: string | null = null;

  constructor(private log: LogFn) {
    super();
  }

  start(): void {
    if (!app.isPackaged || process.env.REFRAME_E2E) return;
    // A deb is updated by the package manager; only the AppImage updates itself.
    if (process.platform === 'linux' && !process.env.APPIMAGE) return;
    // macOS installs updates only into signed apps, which reframe's builds are not (yet).
    if (process.platform === 'darwin') return this.log('info', 'updates need a signed build on macOS, not checking');

    const { autoUpdater } = electronUpdater;
    autoUpdater.logger = {
      info: (message?: unknown) => this.log('info', `updater: ${String(message)}`),
      warn: (message?: unknown) => this.log('warn', `updater: ${String(message)}`),
      error: (message?: unknown) => this.log('warn', `updater: ${String(message)}`),
      debug: () => {},
    };
    autoUpdater.autoDownload = true;
    autoUpdater.autoInstallOnAppQuit = true;
    autoUpdater.on('update-downloaded', (info) => {
      this.ready = info.version;
      this.log('info', 'update downloaded, installs on the next quit', { version: info.version });
      this.emit('ready', info.version);
    });
    const check = () => {
      autoUpdater.checkForUpdates().catch((error: unknown) => {
        this.log('warn', 'update check failed', { error: String(error) });
      });
    };
    setTimeout(check, FIRST_CHECK_MS).unref();
    setInterval(check, CHECK_EVERY_MS).unref();
  }

  /** Quits (the caller has ended the sessions), installs the downloaded update and starts reframe again. */
  install(): void {
    electronUpdater.autoUpdater.quitAndInstall(false, true);
  }
}
