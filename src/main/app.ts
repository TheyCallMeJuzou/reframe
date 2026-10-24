// Wires state, windows, sessions, hooks, tray and IPC together. Main owns the state: windows send
// intents, main applies them, saves, and pushes the new view to the affected window.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import {
  app,
  dialog,
  ipcMain,
  Notification,
  screen,
  shell,
  type IpcMainEvent,
  type IpcMainInvokeEvent,
} from 'electron';
import {
  addTile,
  applyLayoutAction,
  dropSession,
  isDropTarget,
  isLayoutAction,
  isWindowEmpty,
  windowHasSession,
} from '@shared/actions';
import { translate, type Locale } from '@shared/i18n';
import { leaves, tile } from '@shared/layout';
import {
  IPC,
  type NewSessionRequest,
  type NewSessionResult,
  type SessionView,
  type TileSize,
  type TransferRequest,
  type WindowView,
} from '@shared/ipc';
import { PERMISSION_MODES } from '@shared/launch';
import { emptyWindow, type AppState, type SessionRecord, type WindowState } from '@shared/state';
import { claudeHome } from './claude/registry';
import { resolveClaude, type ClaudeBinary } from './claude/resolve';
import { HookServer } from './hooks/server';
import { PtyHostClient } from './ptyhost-client';
import { SessionManager, type IdentityPatch, type NotifyKind } from './sessions';
import { StateStore } from './state/store';
import { createTray } from './tray';
import { WindowManager } from './windows';

export interface AppAssets {
  icon: string;
  trayIcon: string;
  trayIcon2x: string;
}

const MAX_RECENT_FOLDERS = 8;

export class ReframeApp {
  private store = new StateStore(path.join(app.getPath('userData'), 'state.json'));
  private state!: AppState;
  private hooks = new HookServer();
  private host = new PtyHostClient();
  private sessions!: SessionManager;
  private windows: WindowManager;
  private refreshTray: () => void = () => {};
  private shuttingDown = false;

  constructor(
    private locale: Locale,
    private assets: AppAssets,
  ) {
    this.windows = new WindowManager(assets.icon);
  }

  async start(): Promise<void> {
    this.state = this.store.load().state;
    if (this.state.windows.length === 0) this.state.windows.push(emptyWindow(crypto.randomUUID()));

    await this.hooks.start();
    this.host.start();
    this.sessions = new SessionManager({
      host: this.host,
      hooks: this.hooks,
      claudeHome: claudeHome(),
      settingsDir: path.join(app.getPath('userData'), 'hooks'),
      env: process.env,
      resolveClaude: findClaude,
    });
    this.sessions.on('changed', (id) => this.pushSession(id));
    this.sessions.on('identity', (id, patch) => this.updateIdentity(id, patch));
    this.sessions.on('notify', (id, kind) => this.notify(id, kind));
    this.sessions.on('ended', (id) => this.updateSession(id, { lifecycle: 'ended' }));
    await this.sessions.cleanupOrphans(Object.values(this.state.sessions));

    this.registerIpc();
    this.openWindows();
    this.resumeSessions();
    this.createTray();
    this.save();
  }

  /** Shows the windows again, e.g. when reframe is launched a second time. */
  revealWindows(): void {
    for (const w of this.state.windows) this.windows.show(w.id);
  }

  /** Ends all sessions the way a user would, saves, and exits. Sessions stay open and resume next time. */
  async shutdown(): Promise<void> {
    if (this.shuttingDown) return;
    this.shuttingDown = true;
    this.windows.quitting = true;
    this.save();
    await this.sessions.stopAll();
    this.sessions.dispose();
    this.host.stop();
    this.hooks.stop();
    this.save();
  }

  // ---------- windows and views ----------

  private openWindows(): void {
    const anyVisible = this.state.windows.some((w) => !w.hidden);
    this.state.windows.forEach((w, i) => this.windows.open(w, !w.hidden || (!anyVisible && i === 0)));
    this.windows.on('bounds', (id, bounds, maximized) => {
      this.mutateWindow(id, (w) => ({ ...w, bounds, maximized }), 'soon');
    });
    this.windows.on('visibility', (id, hidden) => {
      this.mutateWindow(id, (w) => ({ ...w, hidden }), 'soon');
      this.refreshTray();
    });
  }

  private view(windowId: string): WindowView | null {
    const win = this.state.windows.find((w) => w.id === windowId);
    if (!win) return null;
    const sessions: Record<string, SessionView> = {};
    for (const id of [...leaves(win.layout), ...win.dock.map((d) => d.sessionId)]) {
      const record = this.state.sessions[id];
      if (record) sessions[id] = this.sessionView(record);
    }
    return { windowId, locale: this.locale, layout: win.layout, dock: win.dock, sessions };
  }

  private sessionView(record: SessionRecord): SessionView {
    const runtime = this.sessions.status(record.id);
    const status = runtime?.status ?? (record.lifecycle === 'ended' ? 'ended' : 'starting');
    return {
      id: record.id,
      label: record.name || this.sessions.registryName(record.id) || path.basename(record.cwd) || record.cwd,
      cwd: record.cwd,
      status,
      lifecycle: record.lifecycle,
      error: runtime?.error,
    };
  }

  private push(windowId: string): void {
    const view = this.view(windowId);
    if (view) this.windows.send(windowId, IPC.view, view);
  }

  private pushSession(sessionId: string): void {
    const win = this.windowOf(sessionId);
    if (win) this.push(win.id);
  }

  private windowOf(sessionId: string): WindowState | undefined {
    return this.state.windows.find((w) => windowHasSession(w, sessionId));
  }

  private findWindow(windowId: string): WindowState | undefined {
    return this.state.windows.find((w) => w.id === windowId);
  }

  /** Moves a session's tile from another window into `toId`; without a target it fills the window. */
  private transferTile(toId: string, { sessionId, fromWindowId, target }: TransferRequest): void {
    const from = this.findWindow(fromWindowId);
    if (!from || !this.findWindow(toId) || from.id === toId || !windowHasSession(from, sessionId)) return;
    this.state = {
      ...this.state,
      windows: this.state.windows.map((w) => {
        if (w.id === from.id) return dropSession(w, sessionId);
        if (w.id === toId) return addTile(w, sessionId, target ?? undefined);
        return w;
      }),
    };
    this.save();
    this.push(from.id);
    this.push(toId);
    this.closeIfEmpty(from.id);
    this.windows.show(toId);
    this.windows.send(toId, IPC.focusTile, sessionId);
    this.refreshTray();
  }

  /** Turns a tile into a window of its own at a screen point, unless the point is over one of our windows. */
  private tearOut(fromId: string, sessionId: string, point: { x: number; y: number }, size?: TileSize): boolean {
    const from = this.findWindow(fromId);
    if (!from || !windowHasSession(from, sessionId) || this.windows.windowAt(point)) return false;
    const clamp = (v: number, min: number, max: number) => Math.round(Math.min(max, Math.max(min, v)));
    const win: WindowState = {
      ...emptyWindow(crypto.randomUUID()),
      // Room for the window frame, toolbar and padding around the tile; the title bar lands under the pointer.
      bounds: {
        x: Math.round(point.x - 60),
        y: Math.round(point.y - 16),
        width: clamp((size?.width ?? 800) + 24, 480, 2400),
        height: clamp((size?.height ?? 500) + 90, 320, 1600),
      },
      layout: tile(sessionId),
    };
    this.state = {
      ...this.state,
      windows: [...this.state.windows.map((w) => (w.id === from.id ? dropSession(w, sessionId) : w)), win],
    };
    this.save();
    this.push(from.id);
    this.windows.open(win);
    this.closeIfEmpty(from.id);
    this.refreshTray();
    return true;
  }

  /** Test hook (REFRAME_E2E): tears a session out at a given point instead of the pointer position. */
  tearOutSession(sessionId: string, point: { x: number; y: number }): boolean {
    const from = this.windowOf(sessionId);
    return from ? this.tearOut(from.id, sessionId, point) : false;
  }

  /** Secondary windows go away once nothing is left in them; the first window always stays. */
  private closeIfEmpty(windowId: string): void {
    const win = this.findWindow(windowId);
    if (!win || win.id === this.state.windows[0]?.id || !isWindowEmpty(win)) return;
    this.state = { ...this.state, windows: this.state.windows.filter((w) => w.id !== windowId) };
    this.save();
    this.windows.destroy(windowId);
    this.refreshTray();
  }

  private mutateWindow(
    windowId: string,
    change: (w: WindowState) => WindowState,
    persist: 'now' | 'soon' = 'now',
  ): void {
    this.state = {
      ...this.state,
      windows: this.state.windows.map((w) => (w.id === windowId ? change(w) : w)),
    };
    if (persist === 'now') {
      this.save();
      this.push(windowId);
    } else {
      this.store.saveSoon(this.state);
    }
  }

  private save(): void {
    this.store.save(this.state);
  }

  // ---------- sessions ----------

  private resumeSessions(): void {
    for (const record of Object.values(this.state.sessions)) {
      if (record.lifecycle !== 'open') continue;
      if (this.windowOf(record.id)) this.sessions.start(record, 'resume');
      else this.updateSession(record.id, { lifecycle: 'ended' }, false);
    }
  }

  private updateSession(sessionId: string, patch: Partial<SessionRecord>, persist = true): void {
    const record = this.state.sessions[sessionId];
    if (!record) return;
    this.state = { ...this.state, sessions: { ...this.state.sessions, [sessionId]: { ...record, ...patch } } };
    if (persist) this.save();
    this.pushSession(sessionId);
  }

  private updateIdentity(sessionId: string, patch: IdentityPatch): void {
    const record = this.state.sessions[sessionId];
    if (!record) return;
    const next: IdentityPatch = { ...patch };
    // A new claude session id (after /clear) starts unused unless the same update says otherwise.
    if (patch.claudeSessionId && patch.claudeSessionId !== record.claudeSessionId && patch.used === undefined) {
      next.used = false;
    }
    const changed = (Object.keys(next) as (keyof IdentityPatch)[]).some((k) => next[k] !== record[k]);
    if (changed) this.updateSession(sessionId, { ...next, lastActiveAt: Date.now() });
  }

  private createSession(windowId: string, request: NewSessionRequest): NewSessionResult {
    const cwd = request.cwd.trim();
    if (!isDirectory(cwd)) return { ok: false, error: 'folderMissing' };
    if (!findClaude()) {
      return { ok: false, error: 'claudeNotFound' };
    }
    const now = Date.now();
    const record: SessionRecord = {
      id: crypto.randomUUID(),
      claudeSessionId: crypto.randomUUID(),
      cwd,
      name: request.name?.trim() || undefined,
      launch: {
        model: request.model?.trim() || undefined,
        permissionMode: PERMISSION_MODES.find((m) => m === request.permissionMode),
        worktree: request.worktree || undefined,
        worktreeName: (request.worktree && request.worktreeName?.trim()) || undefined,
      },
      lifecycle: 'open',
      createdAt: now,
      lastActiveAt: now,
    };
    const key = (p: string) => (process.platform === 'win32' ? p.toLowerCase() : p);
    this.state = {
      ...this.state,
      sessions: { ...this.state.sessions, [record.id]: record },
      recentFolders: [cwd, ...this.state.recentFolders.filter((f) => key(f) !== key(cwd))].slice(0, MAX_RECENT_FOLDERS),
    };
    this.mutateWindow(windowId, (w) => addTile(w, record.id, request.target));
    this.sessions.start(record, 'new');
    return { ok: true };
  }

  private restartSession(sessionId: string, fresh: boolean): void {
    const record = this.state.sessions[sessionId];
    if (!record || this.sessions.isRunning(sessionId)) return;
    const next: SessionRecord = {
      ...record,
      ...(fresh ? { claudeSessionId: crypto.randomUUID() } : {}),
      lifecycle: 'open',
      lastActiveAt: Date.now(),
    };
    this.state = { ...this.state, sessions: { ...this.state.sessions, [sessionId]: next } };
    this.save();
    this.sessions.start(next, fresh ? 'new' : 'resume');
    this.pushSession(sessionId);
  }

  private endSession(windowId: string, sessionId: string): void {
    if (!this.state.sessions[sessionId]) return;
    void this.sessions.stop(sessionId);
    this.state = {
      ...this.state,
      sessions: { ...this.state.sessions, [sessionId]: { ...this.state.sessions[sessionId]!, lifecycle: 'ended' } },
    };
    this.mutateWindow(windowId, (w) => dropSession(w, sessionId));
    this.closeIfEmpty(windowId);
  }

  // ---------- notifications and tray ----------

  private notify(sessionId: string, kind: NotifyKind): void {
    const win = this.windowOf(sessionId);
    const record = this.state.sessions[sessionId];
    if (!win || !record || !Notification.isSupported() || process.env.REFRAME_DISABLE_NOTIFICATIONS) return;
    const collapsed = win.dock.some((d) => d.sessionId === sessionId);
    if (this.windows.isAttentive(win.id) && !collapsed) return;
    const label = this.sessionView(record).label;
    const notification = new Notification({
      title: translate(this.locale, kind === 'done' ? 'notify.done' : 'notify.waiting', { label }),
      body: translate(this.locale, kind === 'done' ? 'notify.doneBody' : 'notify.waitingBody'),
      icon: this.assets.icon,
    });
    notification.on('click', () => this.focusSession(sessionId));
    notification.show();
  }

  private focusSession(sessionId: string): void {
    const win = this.windowOf(sessionId);
    if (!win) return;
    this.windows.show(win.id);
    if (win.dock.some((d) => d.sessionId === sessionId)) {
      this.mutateWindow(win.id, (w) => applyLayoutAction(w, { type: 'tile/expand', sessionId }));
    }
    this.windows.send(win.id, IPC.focusTile, sessionId);
  }

  private createTray(): void {
    const t = (key: Parameters<typeof translate>[1], params?: Record<string, string | number>) =>
      translate(this.locale, key, params);
    const { refresh } = createTray(this.assets.trayIcon, this.assets.trayIcon2x, {
      tooltip: t('tray.tooltip'),
      newSessionLabel: t('tray.newSession'),
      quitLabel: t('tray.quit'),
      windows: () =>
        this.state.windows.map((w, i) => {
          const labels = [...leaves(w.layout), ...w.dock.map((d) => d.sessionId)]
            .map((id) => this.state.sessions[id])
            .filter((r): r is SessionRecord => !!r)
            .map((r) => this.sessionView(r).label);
          const names = labels.length > 3 ? `${labels.slice(0, 3).join(', ')}, …` : labels.join(', ');
          const label = t('tray.window', { n: i + 1 }) + (names ? ` – ${names}` : '');
          return { id: w.id, label, visible: !w.hidden };
        }),
      toggleWindow: (id, visible) => (visible ? this.windows.show(id) : this.windows.get(id)?.hide()),
      showPrimary: () => this.windows.show(this.state.windows[0]!.id),
      newSession: () => {
        const id = this.state.windows[0]!.id;
        this.windows.show(id);
        this.windows.send(id, IPC.openNewSession, null);
      },
      quit: () => app.quit(),
    });
    this.refreshTray = refresh;
  }

  // ---------- IPC ----------

  private registerIpc(): void {
    const windowId = (event: IpcMainEvent | IpcMainInvokeEvent) => this.windows.idOf(event.sender);

    ipcMain.handle(IPC.getView, (event) => {
      const id = windowId(event);
      return id ? this.view(id) : null;
    });
    ipcMain.on(IPC.dispatch, (event, action: unknown) => {
      const id = windowId(event);
      if (!id || !isLayoutAction(action)) return;
      this.mutateWindow(id, (w) => applyLayoutAction(w, action), action.type === 'split/resize' ? 'soon' : 'now');
    });
    ipcMain.handle(IPC.createSession, (event, request: NewSessionRequest) => {
      const id = windowId(event);
      if (!id || !request || typeof request.cwd !== 'string') return { ok: false, error: 'folderMissing' };
      return this.createSession(id, request);
    });
    ipcMain.on(IPC.restartSession, (event, sessionId: unknown, fresh: unknown) => {
      const id = windowId(event);
      const win = this.state.windows.find((w) => w.id === id);
      if (typeof sessionId === 'string' && win && windowHasSession(win, sessionId)) {
        this.restartSession(sessionId, fresh === true);
      }
    });
    ipcMain.on(IPC.endSession, (event, sessionId: unknown) => {
      const id = windowId(event);
      if (id && typeof sessionId === 'string') this.endSession(id, sessionId);
    });
    ipcMain.on(IPC.attachTerminal, (event, sessionId: unknown) => {
      const id = windowId(event);
      const win = this.state.windows.find((w) => w.id === id);
      if (typeof sessionId === 'string' && win && windowHasSession(win, sessionId)) {
        this.host.attach(sessionId, event.sender);
      }
    });
    ipcMain.handle(IPC.pickFolder, async (event) => {
      const win = this.windows.get(windowId(event) ?? '');
      const options = { properties: ['openDirectory' as const] };
      const result = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options);
      return result.canceled ? null : (result.filePaths[0] ?? null);
    });
    ipcMain.on(IPC.transferTile, (event, request: Partial<TransferRequest> | null) => {
      const id = windowId(event);
      if (!id || !request || typeof request.sessionId !== 'string' || typeof request.fromWindowId !== 'string') return;
      if (request.target !== null && !isDropTarget(request.target)) return;
      this.transferTile(id, {
        sessionId: request.sessionId,
        fromWindowId: request.fromWindowId,
        target: request.target,
      });
    });
    ipcMain.on(IPC.tearOut, (event, sessionId: unknown, size: Partial<TileSize> | null) => {
      const id = windowId(event);
      if (!id || typeof sessionId !== 'string') return;
      const valid = !!size && Number.isFinite(size.width) && Number.isFinite(size.height);
      this.tearOut(id, sessionId, screen.getCursorScreenPoint(), valid ? (size as TileSize) : undefined);
    });
    ipcMain.on(IPC.openExternal, (_event, url: unknown) => {
      if (typeof url === 'string' && /^https?:\/\//i.test(url)) void shell.openExternal(url);
    });
    ipcMain.handle(IPC.recentFolders, () => this.state.recentFolders.filter(isDirectory));
    ipcMain.handle(IPC.folderExists, (_event, folder: unknown) => typeof folder === 'string' && isDirectory(folder));
  }
}

/** claude from PATH, or REFRAME_CLAUDE_BIN (tests: a .js fake run by REFRAME_NODE). */
function findClaude(): ClaudeBinary | null {
  const node = process.env.REFRAME_NODE;
  return resolveClaude({
    env: process.env,
    platform: process.platform,
    override: process.env.REFRAME_CLAUDE_BIN,
    nodeRunner: node ? { file: node, env: {} } : undefined,
  });
}

function isDirectory(folder: string): boolean {
  try {
    return fs.statSync(folder).isDirectory();
  } catch {
    return false;
  }
}
