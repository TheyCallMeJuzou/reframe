// Updates from GitHub Releases (electron-updater), for installed builds only. Checks a while after the start
// and then every few hours, downloads in the background, and installs on the next quit, or right away on
// "Restart to update" (sessions end the usual way first and resume after the restart).
// An update never quits or restarts reframe by itself, however long it waits: that would end the sessions running
// in it, maybe in the middle of a long task. Installing stays the user's call (quitting, or "Restart to update").
// The portable copy cannot replace itself (the installer would make a second, installed copy): it only checks,
// and tells when there is a newer version to download.
import { EventEmitter } from 'node:events';
import { app } from 'electron';
import electronUpdater, { type AppUpdater } from 'electron-updater';
import type { LogFn } from './log';

const FIRST_CHECK_MS = 30_000;
const CHECK_EVERY_MS = 6 * 60 * 60_000;

type Events = {
  /** Downloaded, installs on the next quit (installed copy). */
  ready: [version: string];
  /** There is a newer version, to be downloaded by hand (portable copy). */
  available: [version: string];
};

/** The parts of electron-updater's updater this uses. */
export type Updater = Pick<AppUpdater, 'autoDownload' | 'autoInstallOnAppQuit' | 'logger' | 'on' | 'checkForUpdates'>;

export class Updates extends EventEmitter<Events> {
  /** Version downloaded and waiting to be installed. */
  ready: string | null = null;
  /** Newer version a portable copy can download. */
  available: string | null = null;

  constructor(
    private log: LogFn,
    private portable = false,
  ) {
    super();
  }

  start(): void {
    if (!app.isPackaged || process.env.REFRAME_E2E) return;
    // A deb is updated by the package manager; only the AppImage updates itself.
    if (process.platform === 'linux' && !process.env.APPIMAGE) return;
    // macOS installs updates only into signed apps, which reframe's builds are not (yet).
    if (process.platform === 'darwin') return this.log('info', 'updates need a signed build on macOS, not checking');
    this.watch(electronUpdater.autoUpdater);
  }

  /** Sets `updater` up for this copy and checks a while after the start, then every few hours. */
  watch(updater: Updater): void {
    updater.logger = {
      info: (message?: unknown) => this.log('info', `updater: ${String(message)}`),
      warn: (message?: unknown) => this.log('warn', `updater: ${String(message)}`),
      error: (message?: unknown) => this.log('warn', `updater: ${String(message)}`),
      debug: () => {},
    };
    updater.autoDownload = !this.portable;
    updater.autoInstallOnAppQuit = !this.portable;
    updater.on('update-downloaded', (info) => {
      this.ready = info.version;
      this.log('info', 'update downloaded, installs on the next quit', { version: info.version });
      this.emit('ready', info.version);
    });
    if (this.portable) {
      updater.on('update-available', (info) => {
        if (this.available === info.version) return;
        this.available = info.version;
        this.log('info', 'update available for the portable copy, to download by hand', { version: info.version });
        this.emit('available', info.version);
      });
    }
    const check = () => {
      updater.checkForUpdates().catch((error: unknown) => {
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
