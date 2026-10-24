// Creates BrowserWindows for persisted windows. Closing a window only hides it: its sessions keep
// running and it comes back from the tray. Bounds changes are reported so they can be saved.
import { EventEmitter } from 'node:events';
import path from 'node:path';
import { BrowserWindow, screen, type WebContents } from 'electron';
import type { Bounds, WindowState } from '@shared/state';

type Events = {
  bounds: [windowId: string, bounds: Bounds, maximized: boolean];
  visibility: [windowId: string, hidden: boolean];
};

const DEFAULT_SIZE = { width: 1280, height: 800 };

export class WindowManager extends EventEmitter<Events> {
  private windows = new Map<string, BrowserWindow>();
  /** Set while the app shuts down, so closing really closes. */
  quitting = false;

  constructor(private icon: string) {
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

    // maximize() also shows a window, so it waits until the window is meant to be visible.
    let maximizePending = state.maximized;
    const applyMaximize = () => {
      if (maximizePending) win.maximize();
      maximizePending = false;
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
      win.hide();
    });
    win.on('show', () => this.emit('visibility', state.id, false));
    win.on('hide', () => this.emit('visibility', state.id, true));
    const report = () => {
      if (win.isDestroyed() || win.isMinimized()) return;
      this.emit('bounds', state.id, win.isMaximized() ? win.getNormalBounds() : win.getBounds(), win.isMaximized());
    };
    win.on('resize', report);
    win.on('move', report);
    win.on('maximize', report);
    win.on('unmaximize', report);
    win.on('closed', () => this.windows.delete(state.id));

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

  /** Closes a window for good (not just hidden), e.g. an empty secondary window. */
  destroy(windowId: string): void {
    const win = this.windows.get(windowId);
    this.windows.delete(windowId);
    if (win && !win.isDestroyed()) win.destroy();
  }

  /** The visible window under a screen point, if any. */
  windowAt(point: { x: number; y: number }): string | undefined {
    for (const [id, win] of this.windows) {
      if (win.isDestroyed() || !win.isVisible() || win.isMinimized()) continue;
      const b = win.getBounds();
      if (point.x >= b.x && point.x < b.x + b.width && point.y >= b.y && point.y < b.y + b.height) return id;
    }
    return undefined;
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
