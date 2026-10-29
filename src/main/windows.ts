// Creates BrowserWindows for persisted windows. Closing a window does not close it but is reported (closeRequested):
// the app hides it into the tray, its sessions running on, or ends them. Bounds changes are reported so they can be
// saved.
import { EventEmitter } from 'node:events';
import path from 'node:path';
import { app, BrowserWindow, screen, type WebContents } from 'electron';
import type { Bounds, WindowState } from '@shared/state';
import { attentionBadge } from './images';
import type { LogFn } from './log';

type Events = {
  bounds: [windowId: string, bounds: Bounds, maximized: boolean];
  visibility: [windowId: string, hidden: boolean];
  fullscreen: [windowId: string, fullscreen: boolean];
  /** The user closes the window (X, Alt+F4); it stays until the app hides or destroys it. */
  closeRequested: [windowId: string];
  /** Windows is shutting down, restarting or logging off (it asks every window first, then tells them). */
  systemEnding: [];
};

const DEFAULT_SIZE = { width: 1280, height: 800 };
/** A page that crashed is loaded again, but not more often than this within a minute (it would only crash again). */
const MAX_RELOADS = 3;

export class WindowManager extends EventEmitter<Events> {
  private windows = new Map<string, BrowserWindow>();
  /** When each window last got the focus (a running count): of our windows, the one focused last is in front. */
  private focusedAt = new Map<string, number>();
  private focusCount = 0;
  /** Set while the app shuts down, so closing really closes. */
  quitting = false;

  constructor(
    private icon: string,
    private log: LogFn = () => {},
  ) {
    super();
  }

  open(state: WindowState, show = !state.hidden): BrowserWindow {
    const bounds = visibleBounds(state.bounds);
    const win = new BrowserWindow({
      ...DEFAULT_SIZE,
      ...bounds,
      minWidth: 480,
      minHeight: 320,
      show: false,
      title: 'reframe',
      icon: this.icon,
      backgroundColor: '#1b1c20',
      autoHideMenuBar: true,
      webPreferences: {
        preload: path.join(__dirname, '../preload/index.js'),
        contextIsolation: true,
        sandbox: true,
        nodeIntegration: false,
      },
    });
    this.windows.set(state.id, win);

    // maximize() and full screen also show a window, so they wait until the window is meant to be visible.
    let maximizePending = state.maximized;
    let fullscreenPending = !!state.fullscreen;
    const applyMaximize = () => {
      if (maximizePending) win.maximize();
      if (fullscreenPending) win.setFullScreen(true);
      maximizePending = false;
      fullscreenPending = false;
    };
    win.once('ready-to-show', () => {
      if (!show) return;
      applyMaximize();
      win.show();
    });
    win.once('show', applyMaximize);
    win.on('close', (event) => {
      if (this.quitting) return;
      event.preventDefault();
      this.emit('closeRequested', state.id);
    });
    win.on('show', () => this.emit('visibility', state.id, false));
    win.on('hide', () => this.emit('visibility', state.id, true));
    const report = () => {
      // Full screen is no size of its own: the window keeps the bounds it returns to.
      if (win.isDestroyed() || win.isMinimized() || win.isFullScreen()) return;
      this.emit('bounds', state.id, win.isMaximized() ? win.getNormalBounds() : win.getBounds(), win.isMaximized());
    };
    win.on('resize', report);
    win.on('move', report);
    win.on('maximize', report);
    win.on('unmaximize', report);
    win.on('enter-full-screen', () => this.emit('fullscreen', state.id, true));
    win.on('leave-full-screen', () => this.emit('fullscreen', state.id, false));
    win.on('closed', () => {
      this.windows.delete(state.id);
      this.focusedAt.delete(state.id);
    });
    win.on('focus', () => {
      win.flashFrame(false);
      this.focusedAt.set(state.id, ++this.focusCount);
    });
    win.on('query-session-end', () => this.emit('systemEnding'));
    win.on('session-end', () => this.emit('systemEnding'));

    // A page that crashed is loaded again: it gets its view from main and attaches its terminals anew, their screens
    // coming from the PTY host, so nothing is lost. Without this the window stays blank.
    const reloads: number[] = [];
    win.webContents.on('render-process-gone', (_event, details) => {
      if (details.reason === 'clean-exit' || win.isDestroyed()) return;
      const now = Date.now();
      while (reloads.length > 0 && now - reloads[0]! > 60_000) reloads.shift();
      const info = { window: state.id, reason: details.reason, exitCode: details.exitCode };
      if (reloads.length >= MAX_RELOADS) return this.log('error', 'window keeps crashing, not loading it again', info);
      reloads.push(now);
      this.log('warn', 'window crashed, loading it again', info);
      win.webContents.reload();
    });

    // Never navigate away from the app or open other windows from page content.
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    win.webContents.on('will-navigate', (event) => event.preventDefault());

    if (process.env.ELECTRON_RENDERER_URL) void win.loadURL(process.env.ELECTRON_RENDERER_URL);
    else void win.loadFile(path.join(__dirname, '../renderer/index.html'));
    return win;
  }

  get(windowId: string): BrowserWindow | undefined {
    return this.windows.get(windowId);
  }

  idOf(contents: WebContents): string | undefined {
    for (const [id, win] of this.windows) if (win.webContents === contents) return id;
    return undefined;
  }

  ids(): string[] {
    return [...this.windows.keys()];
  }

  show(windowId: string): void {
    const win = this.windows.get(windowId);
    if (!win) return;
    if (win.isMinimized()) win.restore();
    win.show();
    win.focus();
  }

  /** Into the tray: the window and its sessions stay, out of sight. */
  hide(windowId: string): void {
    const win = this.windows.get(windowId);
    if (win && !win.isDestroyed()) win.hide();
  }

  toggleFullscreen(windowId: string): void {
    const win = this.windows.get(windowId);
    if (win && !win.isDestroyed()) win.setFullScreen(!win.isFullScreen());
  }

  /** Closes a window for good (not just hidden), e.g. an empty secondary window. */
  destroy(windowId: string): void {
    const win = this.windows.get(windowId);
    this.windows.delete(windowId);
    this.focusedAt.delete(windowId);
    if (win && !win.isDestroyed()) win.destroy();
  }

  /** The visible window under a screen point, if any; where windows overlap, the one in front. */
  windowAt(point: { x: number; y: number }): string | undefined {
    let front: string | undefined;
    for (const [id, win] of this.windows) {
      if (win.isDestroyed() || !win.isVisible() || win.isMinimized()) continue;
      const b = win.getBounds();
      if (point.x < b.x || point.x >= b.x + b.width || point.y < b.y || point.y >= b.y + b.height) continue;
      if (!front || (this.focusedAt.get(id) ?? 0) > (this.focusedAt.get(front) ?? 0)) front = id;
    }
    return front;
  }

  /** Draws attention to a window: its taskbar button flashes until it is looked at (on macOS the Dock icon bounces). */
  flash(windowId: string): void {
    const win = this.windows.get(windowId);
    if (!win || win.isDestroyed()) return;
    if (process.platform === 'darwin') app.dock?.bounce('informational');
    else win.flashFrame(true);
  }

  /** Windows: a badge on the window's taskbar button while `count` sessions in it need the user. */
  badge(windowId: string, count: number, description: string): void {
    const win = this.windows.get(windowId);
    if (process.platform !== 'win32' || !win || win.isDestroyed()) return;
    win.setOverlayIcon(count > 0 ? attentionBadge() : null, description);
  }

  /** Whether the user is looking at this window right now. */
  isAttentive(windowId: string): boolean {
    const win = this.windows.get(windowId);
    return !!win && win.isVisible() && !win.isMinimized() && win.isFocused();
  }

  send(windowId: string, channel: string, payload: unknown): void {
    const win = this.windows.get(windowId);
    if (win && !win.isDestroyed()) win.webContents.send(channel, payload);
  }
}

/** Keeps saved bounds only if the window's top edge is still on a connected display. */
function visibleBounds(bounds: Bounds | null): Partial<Bounds> {
  if (!bounds) return {};
  const titleBar = { x: bounds.x + 40, y: bounds.y, width: Math.max(1, bounds.width - 80), height: 30 };
  const onScreen = screen.getAllDisplays().some(({ workArea: a }) => {
    const w = Math.min(titleBar.x + titleBar.width, a.x + a.width) - Math.max(titleBar.x, a.x);
    const h = Math.min(titleBar.y + titleBar.height, a.y + a.height) - Math.max(titleBar.y, a.y);
    return w > 0 && h > 0;
  });
  return onScreen ? bounds : { width: bounds.width, height: bounds.height };
}
