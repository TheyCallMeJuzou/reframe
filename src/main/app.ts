// Wires state, windows, sessions, hooks, tray and IPC together. Main owns the state: windows send
// intents, main applies them, saves, and pushes the new view to the affected window.
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  app,
  dialog,
  ipcMain,
  Notification,
  powerMonitor,
  screen,
  shell,
  type BrowserWindow,
  type IpcMainEvent,
  type IpcMainInvokeEvent,
  type Menu,
  type MenuItem,
} from 'electron';
import {
  addTile,
  applyLayoutAction,
  dropSession,
  isCollapsed,
  isDropTarget,
  isLayoutAction,
  isWindowEmpty,
  patchGroup,
  putNode,
  takeNode,
  windowGroups,
  windowHasSession,
  windowSessions,
} from '@shared/actions';
import { isGroupColor, nextGroupColor, nextGroupNumber } from '@shared/groups';
import { translate, type Locale, type MessageKey } from '@shared/i18n';
import {
  addToGroup,
  appendNode,
  dissolveGroup as removeGroupFrame,
  findNode,
  groupOf,
  groups,
  groupTile,
  leaves,
  removeFromGroup,
  tile,
  tileKey,
  type DropTarget,
  type GroupNode,
  type LayoutNode,
} from '@shared/layout';
import {
  IPC,
  type NewSessionRequest,
  type NewSessionResult,
  type AppListItem,
  type ForeignCandidate,
  type ForeignDropAnswer,
  type ForeignPointerReport,
  type ClipboardContents,
  type ListItemRef,
  type OpenMode,
  type OpenSessionsRequest,
  type OpenSessionRequest,
  type RunningListItem,
  type SessionList,
  type SessionView,
  type TileSize,
  type TransferRequest,
  type WindowView,
} from '@shared/ipc';
import { PERMISSION_MODES } from '@shared/launch';
import { RELEASES_URL } from '@shared/links';
import {
  CLOSE_WINDOW,
  emptyWindow,
  FONT_SIZE,
  LANGUAGES,
  type CloseWindow,
  type Language,
  type AppState,
  type Preferences,
  type SessionRecord,
  type WindowState,
  type Workspace,
} from '@shared/state';
import { listRunning, TranscriptIndex } from './claude/discovery';
import { claudeHome, readRegistryEntry, transcriptExists, type RegistryEntry } from './claude/registry';
import { resolveClaude, type ClaudeBinary } from './claude/resolve';
import { HookServer } from './hooks/server';
import { consoleHelper } from './console';
import { ForeignWindowWatcher, type ForeignDrop } from './foreign';
import { activeByTitle, isShell, onlyShells } from './foreign-match';
import { createLogger } from './log';
import { isAlive } from './process';
import { PtyHostClient } from './ptyhost-client';
import { SessionManager, type IdentityPatch, type NotifyKind } from './sessions';
import { StateStore } from './state/store';
import { stopExternalClaude } from './takeover';
import { colorMenu, groupMenu, listMenu, tileMenu, workspaceItemMenu, workspaceLabel, workspaceMenu } from './menus';
import { snapshotWindow, windowFromWorkspace } from '@shared/workspaces';
import { parseWslPath, wslWindowsPath, type WslPath } from '@shared/wsl';
import { createTray } from './tray';
import { clipboardImage, readClipboard } from './clipboard';
import { homeRelative } from './paths';
import { Updates } from './updates';
import { autostartEnabled, setAutostart } from './autostart';
import { win32 } from './win32';
import { WindowManager } from './windows';
import { WslDistros } from './wsl/distros';

export interface AppAssets {
  icon: string;
  trayIcon: string;
  trayIcon2x: string;
}

/** Where Claude Code keeps sessions and transcripts: Windows' own, or a WSL distribution's. */
interface ClaudeSource {
  distro?: string;
  /** Claude Code's folder, as Windows reaches it. */
  home: string;
  transcripts: TranscriptIndex;
  /** Whether a process of its (by the id in its registry) runs. */
  alive: (pid: number) => boolean;
  /** A folder it names, as Windows reaches it (WSL names Linux paths). */
  windowsPath: (folder: string) => string;
}

const MAX_RECENT_FOLDERS = 8;
/** How long after the system said it is going down sessions that end still count as ended by it. */
const SYSTEM_ENDING_MS = 60_000;

export class ReframeApp {
  private store = new StateStore(path.join(app.getPath('userData'), 'state.json'));
  private state!: AppState;
  private hooks = new HookServer();
  private host = new PtyHostClient();
  private sessions!: SessionManager;
  private windows: WindowManager;
  private refreshTray: () => void = () => {};
  private shuttingDown = false;
  private log = createLogger(path.join(app.getPath('userData'), 'logs'));
  private claudeHome = claudeHome();
  private transcripts = new TranscriptIndex(this.claudeHome);
  private updates: Updates;
  /** Sessions in WSL folders (Windows only). */
  private wsl = process.platform === 'win32' ? new WslDistros(this.log) : null;

  /** The UI language in use: the one chosen in About, else the system's. */
  private locale: Locale;

  constructor(
    private systemLocale: Locale,
    private assets: AppAssets,
    /** The portable copy (data next to it): it does not update itself. */
    private portable = false,
  ) {
    this.updates = new Updates(this.log, portable, () => this.state.preferences.checkForUpdates);
    this.locale = systemLocale;
    this.windows = new WindowManager(assets.icon);
  }

  private applyLanguage(): void {
    const chosen = this.state.preferences.language;
    this.locale = chosen === 'system' ? this.systemLocale : chosen;
  }

  /** `hidden`: started by the system, so the windows stay in the tray (sessions resume all the same). */
  async start({ hidden = false }: { hidden?: boolean } = {}): Promise<void> {
    const loaded = this.store.load();
    if (loaded.source === 'newer') return this.refuseNewerState();
    if (loaded.rejected) this.log('error', 'state file could not be used, set aside', loaded.rejected);
    this.state = loaded.state;
    if (this.state.windows.length === 0) this.state.windows.push(emptyWindow(crypto.randomUUID()));
    // Closed empty windows that versions up to 0.6.1 kept (and showed again on a second launch) are of no use.
    const leftovers = this.state.windows.filter((w, i) => i > 0 && w.hidden && isWindowEmpty(w));
    if (leftovers.length > 0) {
      this.state = { ...this.state, windows: this.state.windows.filter((w) => !leftovers.includes(w)) };
    }
    this.applyLanguage();
    this.log('info', 'reframe starting', {
      version: app.getVersion(),
      platform: process.platform,
      build: app.isPackaged ? 'installed' : 'development',
      ...(this.portable ? { portable: app.getPath('userData') } : {}),
      hidden,
      state: loaded.source,
      windows: this.state.windows.length,
      ...(leftovers.length > 0 ? { closedEmptyWindowsDropped: leftovers.length } : {}),
      sessions: Object.keys(this.state.sessions).length,
      claude: findClaude()?.source ?? 'not found',
      claudeHome: this.claudeHome,
    });

    await this.hooks.start();
    this.host.start();
    this.sessions = new SessionManager({
      host: this.host,
      hooks: this.hooks,
      claudeHome: this.claudeHome,
      settingsDir: path.join(app.getPath('userData'), 'hooks'),
      env: process.env,
      resolveClaude: findClaude,
      log: this.log,
      wsl: this.wsl ?? undefined,
    });
    this.sessions.on('changed', (id) => this.pushSession(id));
    this.sessions.on('identity', (id, patch) => this.updateIdentity(id, patch));
    this.sessions.on('notify', (id, kind) => this.notify(id, kind));
    this.sessions.on('ended', (id) => this.sessionEnded(id));
    // Distributions with Claude Code are remembered, so the list can tell when one is not running.
    this.wsl?.on('found', (distro) => {
      const known = this.state.wslDistros ?? [];
      if (known.some((d) => d.toLowerCase() === distro.toLowerCase())) return;
      this.state = { ...this.state, wslDistros: [...known, distro] };
      this.save();
    });
    await this.sessions.cleanupOrphans(Object.values(this.state.sessions));

    this.registerIpc();
    // Told first when the system goes down, before it closes the claudes, which then must not count as ended.
    win32()?.shutDownFirst();
    this.windows.on('systemEnding', () => this.systemEnding());
    if (process.platform !== 'win32') powerMonitor.on('shutdown', () => this.systemEnding());
    this.openWindows(hidden);
    this.watchForeignWindows();
    this.resumeSessions();
    this.createTray();
    this.watchUpdates();
    this.save();
  }

  /**
   * The saved state comes from a newer reframe (this one was installed over it, say). Starting from scratch would
   * lose it once anything is saved, so this one says so and quits; the newer version finds everything as it was.
   */
  private refuseNewerState(): void {
    this.log('error', 'state file is from a newer reframe, leaving it alone and quitting', { file: this.store.file });
    // The end-to-end tests cannot click a native dialog away; the log tells them.
    if (!process.env.REFRAME_E2E) {
      dialog.showErrorBox(this.t('state.newerTitle'), this.t('state.newerMessage', { file: this.store.file }));
    }
    app.exit(1);
  }

  /**
   * Shows the windows again, e.g. when reframe is launched a second time: the same ones a start shows, so windows
   * the user closed stay in the tray.
   */
  revealWindows(): void {
    for (const w of this.windowsToShow()) this.windows.show(w.id);
  }

  /** Another build of reframe (installed vs. development) was started and handed over to this one. */
  otherBuildStarted(other: 'installed' | 'development'): void {
    const own = app.isPackaged ? 'installed' : 'development';
    this.log('info', 'another build was started, this one keeps running', { started: other, running: own });
    const browserWindow = this.windows.get(this.state.windows[0]!.id);
    const options = {
      type: 'info' as const,
      title: this.t('otherBuild.title'),
      message: this.t('otherBuild.message', {
        started: this.t(`otherBuild.${other}`),
        running: this.t(`otherBuild.${own}`),
      }),
      detail: this.t('otherBuild.detail'),
    };
    void (browserWindow ? dialog.showMessageBox(browserWindow, options) : dialog.showMessageBox(options));
  }

  /** Ends all sessions the way a user would, saves, and exits. Sessions stay open and resume next time. */
  async shutdown(): Promise<void> {
    if (this.shuttingDown) return;
    this.shuttingDown = true;
    this.windows.quitting = true;
    this.log('info', 'reframe quitting, ending sessions');
    this.save();
    await this.sessions.stopAll();
    this.sessions.dispose();
    this.host.stop();
    this.hooks.stop();
    this.wsl?.release();
    this.save();
    this.log('info', 'reframe stopped');
  }

  /**
   * The system is shutting down, restarting or logging off. Sessions it ends now were not ended by the user: they
   * stay open and resume on the next start. Should the system not go down after all (another program objected),
   * the ones that ended meanwhile count as ended a minute later.
   */
  systemEnding(): void {
    if (!this.systemEndingTimer) this.log('info', 'the system is going down, sessions it ends stay open');
    clearTimeout(this.systemEndingTimer);
    this.systemEndingTimer = setTimeout(() => {
      this.systemEndingTimer = undefined;
      for (const id of this.endedWhileSystemEnding) {
        if (!this.sessions.isRunning(id)) this.updateSession(id, { lifecycle: 'ended' });
      }
      this.endedWhileSystemEnding.clear();
    }, SYSTEM_ENDING_MS);
    this.save();
  }
  private systemEndingTimer: NodeJS.Timeout | undefined;
  private endedWhileSystemEnding = new Set<string>();

  /** A session ended by itself (not by reframe): it stays in its tile as ended, unless the system ended it. */
  private sessionEnded(sessionId: string): void {
    if (this.systemEndingTimer) {
      this.endedWhileSystemEnding.add(sessionId);
      this.log('info', 'session ended while the system goes down, stays open', { session: sessionId });
      return;
    }
    this.updateSession(sessionId, { lifecycle: 'ended' });
  }

  // ---------- windows and views ----------

  /** The windows that were open (not closed into the tray); the first one if all were closed. */
  private windowsToShow(): WindowState[] {
    const open = this.state.windows.filter((w) => !w.hidden);
    return open.length > 0 ? open : this.state.windows.slice(0, 1);
  }

  private openWindows(hidden: boolean): void {
    const shown = new Set(this.windowsToShow());
    for (const w of this.state.windows) this.windows.open(w, !hidden && shown.has(w));
    this.windows.on('bounds', (id, bounds, maximized) => {
      this.mutateWindow(id, (w) => ({ ...w, bounds, maximized }), 'soon');
    });
    this.windows.on('fullscreen', (id, fullscreen) => {
      this.mutateWindow(id, (w) => ({ ...w, fullscreen }), 'soon');
      this.push(id); // the window shows how to leave full screen
    });
    this.windows.on('visibility', (id, hidden) => {
      this.mutateWindow(id, (w) => ({ ...w, hidden }), 'soon');
      // A secondary window closed while empty is gone for good, as when its last tile leaves it (after the hide
      // has run its course, not inside it).
      if (hidden && !this.shuttingDown) {
        setImmediate(() => {
          if (!this.shuttingDown && this.findWindow(id)?.hidden) this.closeIfEmpty(id);
        });
      }
      this.refreshTray();
    });
    this.windows.on('closeRequested', (id) => void this.closeWindow(id));
  }

  /**
   * The user closes a window. With sessions in it, reframe asks (unless an answer was remembered): end them, and the
   * window goes (the first one stays, empty, in the tray), or keep them running with the window hidden in the tray.
   * An empty window just closes; while the system goes down nothing is asked.
   */
  private async closeWindow(windowId: string): Promise<void> {
    const win = this.findWindow(windowId);
    if (!win || this.askingToClose.has(windowId)) return;
    const sessions = windowSessions(win);
    let choice: CloseWindow | null = this.state.preferences.closeWindow;
    if (sessions.length === 0 || this.systemEndingTimer) choice = 'keep';
    if (choice === 'ask') {
      this.askingToClose.add(windowId);
      try {
        choice = await this.askToClose(windowId, sessions.length);
      } finally {
        this.askingToClose.delete(windowId);
      }
    }
    // What is in the window now (the question may have been open for a while).
    const now = this.findWindow(windowId);
    if (!now || !choice) return;
    if (choice === 'end') {
      const ending = windowSessions(now);
      this.log('info', 'window closed, its sessions ended', { window: windowId, sessions: ending.length });
      // Ending the last one closes a secondary window for good; the first one stays, empty.
      for (const id of ending) this.endSession(windowId, id);
    }
    this.windows.hide(windowId);
  }
  /** Windows whose closing question is open, so a second click on X does not ask twice. */
  private askingToClose = new Set<string>();

  /** End the window's sessions, keep them in the tray, or (null) leave the window open; the answer may be kept. */
  private async askToClose(windowId: string, count: number): Promise<'end' | 'keep' | null> {
    const browserWindow = this.windows.get(windowId);
    const options = {
      type: 'question' as const,
      buttons: [this.t('closeWindow.end'), this.t('closeWindow.keep'), this.t('dialog.cancel')],
      defaultId: 1,
      cancelId: 2,
      title: this.t('closeWindow.title'),
      message: count === 1 ? this.t('closeWindow.messageOne') : this.t('closeWindow.message', { count }),
      detail: this.t('closeWindow.detail'),
      checkboxLabel: this.t('closeWindow.remember'),
    };
    const { response, checkboxChecked } = browserWindow
      ? await dialog.showMessageBox(browserWindow, options)
      : await dialog.showMessageBox(options);
    const choice = response === 0 ? 'end' : response === 1 ? 'keep' : null;
    if (choice && checkboxChecked) {
      this.state = { ...this.state, preferences: { ...this.state.preferences, closeWindow: choice } };
      this.save();
      this.log('info', 'closing windows with sessions from now on', { choice });
      for (const w of this.state.windows) this.push(w.id); // Settings shows it
    }
    return choice;
  }

  private view(windowId: string): WindowView | null {
    const win = this.state.windows.find((w) => w.id === windowId);
    if (!win) return null;
    const sessions: Record<string, SessionView> = {};
    for (const id of windowSessions(win)) {
      const record = this.state.sessions[id];
      if (record) sessions[id] = this.sessionView(record);
    }
    return {
      windowId,
      locale: this.locale,
      workspace: (() => {
        const ws = this.workspaceOf(win);
        return ws ? { id: ws.id, name: ws.name } : null;
      })(),
      fullscreen: !!win.fullscreen,
      language: this.state.preferences.language,
      fontSize: this.state.preferences.fontSize,
      snapToGrid: this.state.preferences.snapToGrid,
      checkForUpdates: this.updates.checks ? this.state.preferences.checkForUpdates : null,
      closeWindow: this.state.preferences.closeWindow,
      systemLocale: this.systemLocale,
      version: app.getVersion(),
      sidebarOpen: !!win.sidebarOpen,
      layout: win.layout,
      dock: win.dock,
      sessions,
    };
  }

  private sessionView(record: SessionRecord): SessionView {
    const runtime = this.sessions.status(record.id);
    const status = runtime?.status ?? (record.lifecycle === 'ended' ? 'ended' : 'starting');
    const where = this.wslPath(record.cwd);
    const info = where && this.wsl!.cached(where.distro);
    return {
      id: record.id,
      label: record.name || this.sessions.registryName(record.id) || path.basename(record.cwd) || record.cwd,
      cwd: record.cwd,
      // A WSL folder as "Ubuntu ~/shop" (its home is known once the distribution was asked).
      cwdLabel: where
        ? `${where.distro} ${info ? homeRelative(where.path, info.home, false) : where.path}`
        : homeRelative(record.cwd, os.homedir(), process.platform === 'win32'),
      status,
      lifecycle: record.lifecycle,
      error: runtime?.error,
      ...(where ? { wsl: { distro: where.distro, mountRoot: info?.mountRoot ?? '/mnt/' } } : {}),
    };
  }

  /** Where a folder is in WSL, if it is in one. */
  private wslPath(folder: string): WslPath | null {
    return this.wsl ? parseWslPath(folder, this.wsl.share) : null;
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

  /** Moves a tile or group from another window into `toId`; without a target it goes to the end. */
  private transferNode(toId: string, { key, fromWindowId, target }: TransferRequest): void {
    const from = this.findWindow(fromWindowId);
    if (!from || !this.findWindow(toId) || from.id === toId) return;
    const { win: rest, node } = takeNode(from, key);
    if (!node) return;
    this.state = {
      ...this.state,
      windows: this.state.windows.map((w) => {
        if (w.id === from.id) return rest;
        if (w.id === toId) return putNode(w, node, target);
        return w;
      }),
    };
    this.save();
    this.log('info', 'moved to another window', { node: key, from: from.id, to: toId });
    this.push(from.id);
    this.push(toId);
    this.closeIfEmpty(from.id);
    this.windows.show(toId);
    this.windows.send(toId, IPC.focusTile, leaves(node)[0]);
    this.refreshTray();
  }

  /**
   * A new, empty window, a little offset from `nearId`'s window. It stays until something leaves it empty again
   * (like a window a tile was torn out into) and comes back after a restart like any other.
   */
  newWindow(nearId?: string): string {
    const near =
      this.windows.get(nearId ?? '')?.getBounds() ?? this.windows.get(this.state.windows[0]!.id)?.getBounds();
    const win: WindowState = {
      ...emptyWindow(crypto.randomUUID()),
      bounds: near ? { ...near, x: near.x + 40, y: near.y + 40 } : null,
    };
    this.state = { ...this.state, windows: [...this.state.windows, win] };
    this.save();
    this.log('info', 'new window', { window: win.id });
    this.windows.open(win);
    this.refreshTray();
    return win.id;
  }

  /** Gives a tile or group a window of its own at a screen point, unless the point is over one of our windows. */
  private tearOut(fromId: string, key: string, point: { x: number; y: number }, size?: TileSize): boolean {
    const from = this.findWindow(fromId);
    if (!from || this.windows.windowAt(point)) return false;
    const { win: rest, node } = takeNode(from, key);
    if (!node) return false;
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
      layout: node,
    };
    this.state = {
      ...this.state,
      windows: [...this.state.windows.map((w) => (w.id === from.id ? rest : w)), win],
    };
    this.save();
    this.log('info', 'torn out into a new window', { node: key, from: from.id, window: win.id });
    this.push(from.id);
    this.windows.open(win);
    this.closeIfEmpty(from.id);
    this.refreshTray();
    return true;
  }

  /** Test hook (REFRAME_E2E): tears a tile or group out at a given point instead of the pointer position. */
  tearOutNode(key: string, point: { x: number; y: number }): boolean {
    const from = this.state.windows.find((w) => findNode(w.layout, key));
    return from ? this.tearOut(from.id, key, point) : false;
  }

  /** Test hook (REFRAME_E2E): tears a session's tile out at a given point. */
  tearOutSession(sessionId: string, point: { x: number; y: number }): boolean {
    return this.tearOutNode(tileKey(sessionId), point);
  }

  /** Secondary windows go away once nothing is left in them; the first window always stays. */
  private closeIfEmpty(windowId: string): void {
    const win = this.findWindow(windowId);
    if (!win || win.id === this.state.windows[0]?.id || !isWindowEmpty(win)) return;
    this.state = { ...this.state, windows: this.state.windows.filter((w) => w.id !== windowId) };
    this.save();
    this.log('info', 'empty window closed', { window: windowId });
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

  private async createSession(windowId: string, request: NewSessionRequest): Promise<NewSessionResult> {
    const cwd = request.cwd.trim();
    if (!(await isFolder(cwd))) return { ok: false, error: 'folderMissing' };
    const where = this.wslPath(cwd);
    if (where ? !(await this.wsl!.info(where.distro))?.claude : !findClaude()) {
      return { ok: false, error: 'claudeNotFound', ...(where ? { distro: where.distro } : {}) };
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
    this.log('info', 'session ended by the user', { session: sessionId });
    void this.sessions.stop(sessionId);
    this.state = {
      ...this.state,
      sessions: { ...this.state.sessions, [sessionId]: { ...this.state.sessions[sessionId]!, lifecycle: 'ended' } },
    };
    this.mutateWindow(windowId, (w) => dropSession(w, sessionId));
    this.closeIfEmpty(windowId);
  }

  // ---------- groups (actions of the native menus; public so end-to-end tests can drive them) ----------

  private windowOfGroup(groupId: string): WindowState | undefined {
    return this.state.windows.find((w) => windowGroups(w).some((g) => g.id === groupId));
  }

  private findGroup(groupId: string): GroupNode | undefined {
    return this.state.windows.flatMap(windowGroups).find((g) => g.id === groupId);
  }

  /** Name ("Group n") and color for a new group: the next free ones. */
  private newGroupLook(): { name: string; color: string } {
    const existing = this.state.windows.flatMap(windowGroups);
    const nameFor = (n: number) => this.t('group.defaultName', { n });
    const number = nextGroupNumber(
      existing.map((g) => g.name),
      nameFor,
    );
    return { name: nameFor(number), color: nextGroupColor(existing.map((g) => g.color)) };
  }

  /** Frames a tile as a new group, named "Group n" with the next free color. */
  createGroup(sessionId: string): void {
    const win = this.windowOf(sessionId);
    if (!win) return;
    const group = this.newGroupLook();
    this.mutateWindow(win.id, (w) => ({ ...w, layout: groupTile(w.layout, sessionId, group) }));
    this.log('info', 'group created', { session: sessionId, ...group });
  }

  addSessionToGroup(sessionId: string, groupId: string): void {
    const win = this.windowOf(sessionId);
    if (win && this.windowOfGroup(groupId)?.id === win.id) {
      this.mutateWindow(win.id, (w) => ({ ...w, layout: addToGroup(w.layout, sessionId, groupId) }));
    }
  }

  removeSessionFromGroup(sessionId: string): void {
    const win = this.windowOf(sessionId);
    if (win) this.mutateWindow(win.id, (w) => ({ ...w, layout: removeFromGroup(w.layout, sessionId) }));
  }

  dissolveGroup(groupId: string): void {
    const win = this.windowOfGroup(groupId);
    if (win) this.mutateWindow(win.id, (w) => ({ ...w, layout: removeGroupFrame(w.layout, groupId) }));
  }

  recolorGroup(groupId: string, color: string): void {
    const win = this.windowOfGroup(groupId);
    if (win && isGroupColor(color)) this.mutateWindow(win.id, (w) => patchGroup(w, groupId, { color }));
  }

  /** Ends every session of the group; like a tile's ×, they stay in the session list. */
  endGroup(groupId: string): void {
    const win = this.windowOfGroup(groupId);
    const group = this.findGroup(groupId);
    if (!win || !group) return;
    this.log('info', 'ending all sessions of a group', { group: groupId, sessions: leaves(group).length });
    for (const sessionId of leaves(group)) this.endSession(win.id, sessionId);
  }

  private showTileMenu(windowId: string, sessionId: string): void {
    const win = this.findWindow(windowId);
    const browserWindow = this.windows.get(windowId);
    if (!win || !browserWindow || !windowHasSession(win, sessionId)) return;
    const group = groupOf(win.layout, tileKey(sessionId));
    const menu = tileMenu(this.t, {
      rename: () => this.windows.send(windowId, IPC.startRenameSession, { sessionId, place: 'tile' }),
      group,
      otherGroups: groups(win.layout).filter((g) => g.id !== group?.id),
      newGroup: () => this.createGroup(sessionId),
      addToGroup: (groupId) => this.addSessionToGroup(sessionId, groupId),
      removeFromGroup: () => this.removeSessionFromGroup(sessionId),
    });
    this.popup(menu, browserWindow);
  }

  /** The list's menu for the selected entries. */
  private showListMenu(windowId: string, items: ListItemRef[]): void {
    const browserWindow = this.windows.get(windowId);
    if (!browserWindow || items.length === 0) return;
    const open = (mode: OpenMode) => () =>
      void this.openSessions(windowId, items, mode, null).catch((error: unknown) =>
        this.log('error', 'opening sessions from the list failed', { error: String(error) }),
      );
    const single = items.length === 1 && items[0]!.kind === 'app' ? items[0]!.sessionId : null;
    const menu = listMenu(this.t, {
      rename: single
        ? () => this.windows.send(windowId, IPC.startRenameSession, { sessionId: single, place: 'list' })
        : undefined,
      count: items.length,
      open: open('open'),
      group: open('group'),
      groupInNewWindow: open('groupInNewWindow'),
    });
    this.popup(menu, browserWindow);
  }

  /** Shows a context menu; end-to-end tests cannot click native menus, so there it is only kept for clickMenuItem. */
  private popup(menu: Menu, browserWindow: BrowserWindow): void {
    this.lastMenu = menu;
    if (!process.env.REFRAME_E2E) menu.popup({ window: browserWindow });
  }
  private lastMenu: Menu | null = null;

  /** Test hook (REFRAME_E2E): clicks the item labeled `label` in the menu shown last. */
  clickMenuItem(label: string): boolean {
    const find = (items: MenuItem[]): MenuItem | undefined =>
      items.reduce<MenuItem | undefined>(
        (found, item) => found ?? (item.label === label ? item : item.submenu ? find(item.submenu.items) : undefined),
        undefined,
      );
    const item = this.lastMenu && find(this.lastMenu.items);
    if (!item) return false;
    item.click();
    return true;
  }

  private showGroupMenu(windowId: string, groupId: string, colorsOnly: boolean): void {
    const browserWindow = this.windows.get(windowId);
    const group = this.findGroup(groupId);
    if (!browserWindow || !group || this.windowOfGroup(groupId)?.id !== windowId) return;
    const recolor = (hex: string) => this.recolorGroup(groupId, hex);
    const menu = colorsOnly
      ? colorMenu(this.t, group, recolor)
      : groupMenu(this.t, group, {
          rename: () => this.windows.send(windowId, IPC.renameGroup, groupId),
          recolor,
          dissolve: () => this.dissolveGroup(groupId),
          endAll: () => this.endGroup(groupId),
        });
    menu.popup({ window: browserWindow });
  }

  // ---------- session list ----------

  private async sessionList(query: string, limit: number): Promise<SessionList> {
    const q = query.trim().toLowerCase();
    const matches = (...fields: (string | undefined)[]) => !q || fields.some((f) => f?.toLowerCase().includes(q));
    const records = Object.values(this.state.sessions);

    const app: AppListItem[] = records
      .map((record): AppListItem => {
        const view = this.sessionView(record);
        const win = this.windowOf(record.id);
        return {
          kind: 'app',
          id: record.id,
          label: view.label,
          cwd: record.cwd,
          status: view.status,
          lifecycle: record.lifecycle,
          windowId: win?.id ?? null,
          windowNumber: win ? this.state.windows.indexOf(win) + 1 : null,
          lastActiveAt: record.lastActiveAt,
        };
      })
      .filter((item) => matches(item.label, item.cwd))
      // Sessions in a window first, then the rest; most recently used first within each.
      .sort((a, b) => Number(!a.windowId) - Number(!b.windowId) || b.lastActiveAt - a.lastActiveAt);

    // Where Claude Code keeps its sessions: on Windows, and in each running WSL distribution that has it.
    const { sources, stopped } = await this.claudeSources();
    const running: RunningListItem[] = sources
      .flatMap((source) =>
        this.externalRunning(source).map((entry): RunningListItem => ({
          kind: 'running',
          pid: entry.pid,
          claudeSessionId: entry.sessionId,
          cwd: source.windowsPath(entry.cwd!),
          name: entry.name,
          ...(source.distro ? { distro: source.distro } : {}),
        })),
      )
      .filter((item) => matches(item.name, item.cwd));

    const ownIds = new Set(records.map((r) => r.claudeSessionId));
    const exclude = new Set([...ownIds, ...running.map((r) => r.claudeSessionId)]);
    const pages = await Promise.all(
      sources.map(async (source) => ({ source, page: await source.transcripts.list({ query, limit, exclude }) })),
    );
    const found = pages
      .flatMap(({ source, page }) =>
        page.items
          .filter((info) => info.cwd)
          .map((info) => ({
            kind: 'found' as const,
            claudeSessionId: info.sessionId,
            cwd: source.windowsPath(info.cwd!),
            title: info.title,
            modifiedAt: info.modifiedAt,
            ...(source.distro ? { distro: source.distro } : {}),
          })),
      )
      .sort((a, b) => b.modifiedAt - a.modifiedAt);
    const workspaces = this.state.workspaces.map((w) => ({
      id: w.id,
      name: w.name,
      sessions: w.sessions.length,
      savedAt: w.savedAt,
      open: !!this.windowOfWorkspace(w.id),
    }));
    const hasMore = found.length > limit || pages.some(({ page }) => page.hasMore);
    return { workspaces, app, running, found: found.slice(0, limit), hasMore, wslStopped: stopped };
  }

  /**
   * Where Claude Code keeps sessions and transcripts: on Windows, and in each running WSL distribution that has it
   * (a stopped one is not started for this; `stopped` names those Claude Code was found in before).
   */
  private async claudeSources(): Promise<{ sources: ClaudeSource[]; stopped: string[] }> {
    const windows = this.ownSource();
    const wsl = this.wsl;
    if (!wsl) return { sources: [windows], stopped: [] };
    const running = await wsl.running();
    const inWsl = await Promise.all(running.map((distro) => this.wslSource(distro)));
    const stopped = (this.state.wslDistros ?? []).filter(
      (d) => !running.some((r) => r.toLowerCase() === d.toLowerCase()),
    );
    return { sources: [windows, ...inWsl.filter((s): s is ClaudeSource => !!s)], stopped };
  }

  /** Claude Code's sessions on this system itself (on Windows: not the ones in WSL). */
  private ownSource(): ClaudeSource {
    return { home: this.claudeHome, transcripts: this.transcripts, alive: isAlive, windowsPath: (folder) => folder };
  }

  /** Claude Code's sessions in a WSL distribution, if it has Claude Code (asking it starts it). */
  private async wslSource(distro: string): Promise<ClaudeSource | null> {
    const wsl = this.wsl!;
    const info = await wsl.info(distro);
    if (!info?.claude) return null;
    const home = wslWindowsPath(distro, info.claudeHome, wsl.share);
    let transcripts = this.wslTranscripts.get(home);
    if (!transcripts) this.wslTranscripts.set(home, (transcripts = new TranscriptIndex(home)));
    return {
      distro,
      home,
      transcripts,
      alive: (pid) => wsl.alive(distro, pid),
      windowsPath: (folder) => wslWindowsPath(distro, folder, wsl.share),
    };
  }
  private wslTranscripts = new Map<string, TranscriptIndex>();

  /** Claudes running outside reframe (with a known folder). */
  private externalRunning(source: ClaudeSource): RegistryEntry[] {
    const records = Object.values(this.state.sessions);
    const ownPids = source.distro
      ? new Set<number>()
      : new Set(records.map((r) => this.sessions.status(r.id)?.pid).filter((p): p is number => !!p));
    const ownIds = new Set(records.map((r) => r.claudeSessionId));
    return listRunning(source.home, { pids: ownPids, sessionIds: ownIds }, source.alive).filter((entry) => entry.cwd);
  }

  /** Where a list entry's Claude Code keeps its sessions: on Windows, or in its WSL distribution. */
  private async sourceOf(item: { distro?: string }): Promise<ClaudeSource | null> {
    if (!item.distro) return this.ownSource();
    return this.wsl ? this.wslSource(item.distro) : null;
  }

  /**
   * The record for a list entry: reframe's own, or a new one for a session found or running elsewhere. One running
   * elsewhere is ended there first (after asking, unless already `confirmed`), so a conversation never runs twice.
   * Null when there is nothing (any more) to open.
   */
  private async recordFor(windowId: string, item: ListItemRef, confirmed: boolean): Promise<SessionRecord | null> {
    if (item.kind === 'app') return this.state.sessions[item.sessionId] ?? null;

    // A conversation reframe already manages is opened as that session, not a second copy.
    const known = Object.values(this.state.sessions).find((r) => r.claudeSessionId === item.claudeSessionId);
    if (known) return known;

    const source = await this.sourceOf(item);
    if (!source) return null;
    if (item.kind === 'found') {
      const info = await source.transcripts.find(item.claudeSessionId);
      if (info?.cwd) return this.adopted(item.claudeSessionId, source.windowsPath(info.cwd), undefined, source.home);
      this.log('warn', 'found session has no folder', { claudeSession: item.claudeSessionId });
      return null;
    }

    const entry = readRegistryEntry(source.home, item.pid);
    if (!entry || entry.sessionId !== item.claudeSessionId || !entry.cwd || !source.alive(item.pid)) {
      this.log('info', 'running session is gone, nothing to take over', { pid: item.pid, wsl: source.distro });
      return null;
    }
    if (!confirmed && !(await this.confirmTakeover(windowId, [entry.name ?? path.basename(entry.cwd)]))) {
      return null;
    }
    this.log('info', 'taking over session from another terminal', {
      pid: item.pid,
      claudeSession: entry.sessionId,
      ...(source.distro ? { wsl: source.distro } : {}),
    });
    // One in WSL is a Linux process: it gets the interrupt as a signal, as from Ctrl+C in its terminal.
    const result = source.distro
      ? await this.wsl!.stopClaude(source.distro, item.pid)
      : await stopExternalClaude(item.pid, this.log);
    this.log(result === 'failed' ? 'warn' : 'info', 'other terminal stopped', { pid: item.pid, result });
    if (result === 'failed') {
      await this.showError(windowId, this.t('takeover.failed'));
      return null;
    }
    // A name the user gave it survives; one Claude made up is made up again.
    const name = entry.nameSource === 'user' ? entry.name : undefined;
    return this.adopted(entry.sessionId, source.windowsPath(entry.cwd), name, source.home);
  }

  /** Asks once before ending sessions that run in other terminals. */
  private async confirmTakeover(windowId: string, labels: string[]): Promise<boolean> {
    const browserWindow = this.windows.get(windowId);
    const options = {
      type: 'question' as const,
      buttons: [this.t('takeover.confirm'), this.t('dialog.cancel')],
      defaultId: 0,
      cancelId: 1,
      title: this.t('takeover.title'),
      message:
        labels.length === 1
          ? this.t('takeover.message', { label: labels[0]! })
          : this.t('takeover.messageMany', { count: labels.length, labels: labels.join(', ') }),
      detail: this.t('takeover.detail'),
    };
    const { response } = browserWindow
      ? await dialog.showMessageBox(browserWindow, options)
      : await dialog.showMessageBox(options);
    return response === 0;
  }

  private async showError(windowId: string, message: string): Promise<void> {
    const browserWindow = this.windows.get(windowId);
    const options = { type: 'error' as const, message };
    if (browserWindow) await dialog.showMessageBox(browserWindow, options);
    else await dialog.showMessageBox(options);
  }

  /** Opens a list entry in `windowId` at `target`; a session already shown somewhere is just brought up. */
  private async openSession(windowId: string, item: ListItemRef, target: DropTarget | null): Promise<void> {
    const record = await this.recordFor(windowId, item, false);
    if (!record) return;
    if (this.windowOf(record.id)) return this.focusSession(record.id);
    this.placeAndResume(windowId, { ...record, lifecycle: 'open' }, target);
  }

  /**
   * Opens several list entries at once: one after the other (at `target`, each below the one before), or together
   * in a new group, in this window or a new one. Sessions running elsewhere are taken over after one question for
   * all of them. Opening sessions that are shown already just brings them up; dropping them somewhere or grouping
   * them moves them there.
   */
  async openSessions(windowId: string, items: ListItemRef[], mode: OpenMode, target: DropTarget | null): Promise<void> {
    let wanted = items;
    const running = items.flatMap((i) => (i.kind === 'running' ? [i] : []));
    if (running.length > 0) {
      const labels = await Promise.all(
        running.map(async (i) => {
          const source = await this.sourceOf(i);
          const entry = source && readRegistryEntry(source.home, i.pid);
          return entry?.name ?? (entry?.cwd ? path.basename(entry.cwd) : String(i.pid));
        }),
      );
      if (!(await this.confirmTakeover(windowId, labels))) wanted = items.filter((i) => i.kind !== 'running');
    }
    const records: SessionRecord[] = [];
    for (const item of wanted) {
      const record = await this.recordFor(windowId, item, true);
      if (record && !records.some((r) => r.id === record.id)) records.push(record);
    }
    if (records.length === 0) return;
    this.log('info', 'opening sessions from the list', { sessions: records.length, mode, at: target ?? 'end' });

    if (mode === 'open' && !target) {
      for (const record of records) {
        if (this.windowOf(record.id)) this.focusSession(record.id);
        else this.placeAndResume(windowId, { ...record, lifecycle: 'open' }, null);
      }
      return;
    }

    const into = mode === 'groupInNewWindow' ? this.newWindow(windowId) : windowId;
    const ids = records.map((r) => r.id);
    this.state = {
      ...this.state,
      sessions: {
        ...this.state.sessions,
        ...Object.fromEntries(records.map((r) => [r.id, { ...(this.state.sessions[r.id] ?? r), lifecycle: 'open' }])),
      },
    };
    // Out of wherever they are shown now; a window left empty by that closes only after they have landed.
    const left = new Set(ids.map((id) => this.detach(id)).filter((w): w is string => !!w && w !== into));
    if (mode === 'open') {
      let next: DropTarget | null = target;
      for (const id of ids) {
        this.mutateWindow(into, (w) => putNode(w, tile(id), next));
        next = { key: tileKey(id), side: 'bottom' };
      }
    } else {
      let child: LayoutNode | null = null;
      for (const id of ids) child = appendNode(child, tile(id));
      const group: GroupNode = { kind: 'group', id: crypto.randomUUID(), ...this.newGroupLook(), child: child! };
      this.mutateWindow(into, (w) => putNode(w, group, target));
      this.log('info', 'group created from the list', { sessions: ids.length, name: group.name });
    }
    for (const id of ids) {
      if (!this.sessions.keepsRunning(id)) this.sessions.start(this.state.sessions[id]!, 'resume');
    }
    for (const id of left) this.closeIfEmpty(id);
    this.windows.show(into);
    this.windows.send(into, IPC.focusTile, ids[0]);
    this.refreshTray();
  }

  /** Takes a session's tile out of the window showing it (a collapsed one is put back first); that window's id. */
  private detach(sessionId: string): string | null {
    const win = this.windowOf(sessionId);
    if (!win) return null;
    let next = win;
    for (const entry of win.dock) {
      if (entry.kind === 'tile' && entry.sessionId === sessionId) {
        next = applyLayoutAction(next, { type: 'tile/expand', sessionId });
      } else if (entry.kind === 'group' && leaves(entry.node).includes(sessionId)) {
        next = applyLayoutAction(next, { type: 'group/expand', groupId: entry.node.id });
      }
    }
    const { win: rest } = takeNode(next, tileKey(sessionId));
    this.mutateWindow(win.id, () => rest);
    return win.id;
  }

  /** A new record for a Claude Code session started outside reframe. */
  /** `claudeHome`: where its Claude Code keeps transcripts (in WSL, its distribution's). */
  private adopted(claudeSessionId: string, cwd: string, name?: string, claudeHome = this.claudeHome): SessionRecord {
    // Without a transcript (no prompt yet) it starts fresh under the same id instead of failing to resume.
    const now = Date.now();
    return {
      id: crypto.randomUUID(),
      claudeSessionId,
      cwd,
      name,
      launch: {},
      lifecycle: 'open',
      createdAt: now,
      lastActiveAt: now,
      used: transcriptExists(claudeHome, claudeSessionId),
    };
  }

  private placeAndResume(windowId: string, record: SessionRecord, target: DropTarget | null): void {
    this.state = { ...this.state, sessions: { ...this.state.sessions, [record.id]: record } };
    this.mutateWindow(windowId, (w) => putNode(w, tile(record.id), target));
    this.log('info', 'session opened from the list', { session: record.id, window: windowId });
    this.sessions.start(record, 'resume');
    this.windows.send(windowId, IPC.focusTile, record.id);
  }

  /** A name the user gives a session (in reframe only); an empty one goes back to the automatic name. */
  renameSession(sessionId: string, name: string): void {
    const record = this.state.sessions[sessionId];
    if (!record) return;
    const trimmed = name.trim().slice(0, 100);
    // Claude Code keeps the name it was started with; renaming is reframe's own (shown instead of it).
    const claudeName = record.claudeName === undefined ? (record.name ?? null) : record.claudeName;
    this.updateSession(sessionId, { name: trimmed || undefined, claudeName });
    this.log('info', 'session renamed', { session: sessionId, automatic: !trimmed });
    this.refreshTray();
  }

  /** Forgets an ended session that is in no window; Claude Code's transcript is left alone. */
  private removeSession(sessionId: string): void {
    if (!this.state.sessions[sessionId] || this.windowOf(sessionId)) return;
    void this.sessions.stop(sessionId); // still ending right after its tile's ×: let it finish in the background
    const { [sessionId]: _removed, ...rest } = this.state.sessions;
    this.state = { ...this.state, sessions: rest };
    this.save();
    fs.rmSync(path.join(app.getPath('userData'), 'hooks', `${sessionId}.json`), { force: true });
    this.log('info', 'session removed from the list', { session: sessionId });
  }

  // ---------- workspaces: windows saved under a name ----------

  /** Saves the window as a new workspace (a snapshot; later changes stay out until it is updated). */
  saveWorkspace(windowId: string, name: string): void {
    const win = this.findWindow(windowId);
    if (!win) return;
    const workspace = snapshotWindow(win, this.state.sessions, {
      id: crypto.randomUUID(),
      name: name.trim().slice(0, 80) || this.t('workspace.defaultName', { n: this.state.workspaces.length + 1 }),
      savedAt: Date.now(),
    });
    this.state = {
      ...this.state,
      workspaces: [...this.state.workspaces, workspace],
      windows: this.state.windows.map((w) => (w.id === windowId ? { ...w, workspaceId: workspace.id } : w)),
    };
    this.save();
    this.push(windowId);
    this.refreshTray();
    this.log('info', 'workspace saved', { workspace: workspace.id, sessions: workspace.sessions.length });
  }

  /** Saves the window again over the workspace it was saved as or opened from. */
  updateWorkspace(windowId: string): void {
    const win = this.findWindow(windowId);
    const old = win && this.workspaceOf(win);
    if (!win || !old) return;
    const workspace = snapshotWindow(win, this.state.sessions, { id: old.id, name: old.name, savedAt: Date.now() });
    this.state = { ...this.state, workspaces: this.state.workspaces.map((w) => (w.id === old.id ? workspace : w)) };
    this.save();
    this.log('info', 'workspace updated', { workspace: old.id, sessions: workspace.sessions.length });
  }

  /**
   * Opens a workspace as a window of its own; if its window is open already, that one comes up. Sessions shown
   * elsewhere move into it, the others resume; sessions that left the list since come back from the snapshot.
   */
  openWorkspace(workspaceId: string): void {
    const workspace = this.state.workspaces.find((w) => w.id === workspaceId);
    if (!workspace) return;
    const open = this.windowOfWorkspace(workspaceId);
    if (open) return this.windows.show(open.id);
    // A window that was the workspace's but shows none of its sessions any more is just a window now.
    this.state = {
      ...this.state,
      windows: this.state.windows.map((w) => (w.workspaceId === workspaceId ? { ...w, workspaceId: undefined } : w)),
    };

    const ids = new Map<string, string>();
    const sessions = { ...this.state.sessions };
    for (const saved of workspace.sessions) {
      const current =
        sessions[saved.id] ?? Object.values(sessions).find((r) => r.claudeSessionId === saved.claudeSessionId);
      const record = current ?? saved;
      sessions[record.id] = { ...record, lifecycle: 'open' };
      ids.set(saved.id, record.id);
    }
    this.state = { ...this.state, sessions };
    const left = new Set([...ids.values()].map((id) => this.detach(id)).filter((w): w is string => !!w));
    const groupIdsInUse = new Set(this.state.windows.flatMap(windowGroups).map((g) => g.id));
    const win = windowFromWorkspace(workspace, crypto.randomUUID(), ids, groupIdsInUse, () => crypto.randomUUID());
    this.state = { ...this.state, windows: [...this.state.windows, win] };
    this.save();
    this.windows.open(win);
    for (const id of ids.values()) {
      if (!this.sessions.keepsRunning(id)) this.sessions.start(this.state.sessions[id]!, 'resume');
    }
    for (const id of left) this.closeIfEmpty(id);
    this.refreshTray();
    this.log('info', 'workspace opened', { workspace: workspaceId, window: win.id, sessions: ids.size });
  }

  renameWorkspace(workspaceId: string, name: string): void {
    const trimmed = name.trim().slice(0, 80);
    if (!trimmed || !this.state.workspaces.some((w) => w.id === workspaceId)) return;
    this.state = {
      ...this.state,
      workspaces: this.state.workspaces.map((w) => (w.id === workspaceId ? { ...w, name: trimmed } : w)),
    };
    this.save();
    for (const w of this.state.windows) if (w.workspaceId === workspaceId) this.push(w.id);
    this.refreshTray();
  }

  /** Forgets a saved workspace (after asking); a window opened from it stays, as a plain window. */
  async deleteWorkspace(windowId: string, workspaceId: string): Promise<void> {
    const workspace = this.state.workspaces.find((w) => w.id === workspaceId);
    if (!workspace) return;
    const browserWindow = this.windows.get(windowId);
    const options = {
      type: 'question' as const,
      buttons: [this.t('workspace.delete'), this.t('dialog.cancel')],
      defaultId: 1,
      cancelId: 1,
      message: this.t('workspace.deleteQuestion', { name: workspace.name }),
      detail: this.t('workspace.deleteDetail'),
    };
    const { response } = browserWindow
      ? await dialog.showMessageBox(browserWindow, options)
      : await dialog.showMessageBox(options);
    if (response !== 0) return;
    this.state = {
      ...this.state,
      workspaces: this.state.workspaces.filter((w) => w.id !== workspaceId),
      windows: this.state.windows.map((w) => (w.workspaceId === workspaceId ? { ...w, workspaceId: undefined } : w)),
    };
    this.save();
    for (const w of this.state.windows) this.push(w.id);
    this.refreshTray();
    this.log('info', 'workspace deleted', { workspace: workspaceId });
  }

  private workspaceItems(): { id: string; name: string; open: boolean }[] {
    return this.state.workspaces.map((w) => ({ id: w.id, name: w.name, open: !!this.windowOfWorkspace(w.id) }));
  }

  /**
   * The workspace a window stands for: the one it was saved as or opened from, as long as it still shows some of
   * that workspace's sessions (a window that has become something else is just a window).
   */
  private workspaceOf(win: WindowState): Workspace | null {
    const workspace = this.state.workspaces.find((w) => w.id === win.workspaceId);
    if (!workspace) return null;
    const ids = new Set(workspace.sessions.flatMap((s) => [s.id, s.claudeSessionId]));
    const shows = windowSessions(win).some(
      (id) => ids.has(id) || ids.has(this.state.sessions[id]?.claudeSessionId ?? ''),
    );
    return shows ? workspace : null;
  }

  private windowOfWorkspace(workspaceId: string): WindowState | undefined {
    return this.state.windows.find((w) => w.workspaceId === workspaceId && this.workspaceOf(w));
  }

  /** The toolbar's Workspaces menu: save this window, update its workspace, open saved ones. */
  private showWorkspaceMenu(windowId: string): void {
    const browserWindow = this.windows.get(windowId);
    const win = this.findWindow(windowId);
    if (!browserWindow || !win) return;
    const current = this.workspaceOf(win);
    const menu = workspaceMenu(this.t, {
      current,
      workspaces: this.workspaceItems(),
      save: () => this.windows.send(windowId, IPC.promptWorkspaceName, null),
      update: () => this.updateWorkspace(windowId),
      open: (id) => this.openWorkspace(id),
    });
    this.popup(menu, browserWindow);
  }

  /** A workspace's menu in the session list. */
  private showWorkspaceItemMenu(windowId: string, workspaceId: string): void {
    const browserWindow = this.windows.get(windowId);
    if (!browserWindow || !this.state.workspaces.some((w) => w.id === workspaceId)) return;
    const menu = workspaceItemMenu(this.t, {
      open: () => this.openWorkspace(workspaceId),
      rename: () => this.windows.send(windowId, IPC.startRenameWorkspace, workspaceId),
      remove: () => void this.deleteWorkspace(windowId, workspaceId),
    });
    this.popup(menu, browserWindow);
  }

  // ---------- terminal windows dragged onto reframe (Windows) ----------

  private foreign: ForeignWindowWatcher | null = null;
  private foreignRequests = new Map<string, (answer: ForeignDropAnswer | null) => void>();
  private takingOver = new Set<string>();

  private watchForeignWindows(): void {
    const api = win32();
    if (!api) return;
    this.foreign = new ForeignWindowWatcher({
      win32: api,
      windowAt: (point) => this.windows.windowAt(point),
      externalSessions: () => this.externalRunning(this.ownSource()),
      log: this.log,
    });
    this.foreign.on('hover', (windowId, point) => {
      this.windows.send(windowId, IPC.foreignDrag, point && this.toContent(windowId, point));
    });
    this.foreign.on('drop', (drop) => {
      void this.takeOverWindow(drop).catch((error: unknown) =>
        this.log('error', 'taking over a terminal window failed', { error: String(error) }),
      );
    });
    try {
      this.foreign.start();
    } catch (error) {
      this.log('warn', 'cannot follow window drags', { error: String(error) });
    }
  }

  private toContent(windowId: string, point: { x: number; y: number }): { x: number; y: number } {
    const bounds = this.windows.get(windowId)?.getContentBounds() ?? { x: 0, y: 0 };
    return { x: point.x - bounds.x, y: point.y - bounds.y };
  }

  /** Asks (in the window it was dropped on) which sessions to take over; resumes them; closes the terminal. */
  private async takeOverWindow(drop: ForeignDrop): Promise<void> {
    // Some terminals report the end of a window move twice; one question per window is enough.
    if (this.takingOver.has(drop.hwnd)) return this.log('info', 'repeated drop of the same terminal window ignored');
    this.takingOver.add(drop.hwnd);
    try {
      await this.takeOverWindowOnce(drop);
    } finally {
      setTimeout(() => this.takingOver.delete(drop.hwnd), 2000);
    }
  }

  private async takeOverWindowOnce(drop: ForeignDrop): Promise<void> {
    this.log('info', 'terminal window dropped on reframe', { title: drop.title, sessions: drop.sessions.length });
    this.windows.show(drop.windowId);
    const active = activeByTitle(drop.title, drop.sessions);
    const candidates: ForeignCandidate[] = drop.sessions.map((s) => ({
      pid: s.pid,
      claudeSessionId: s.sessionId,
      cwd: s.cwd!,
      name: s.name,
      preselected: drop.sessions.length === 1 || s === active,
    }));
    const requestId = crypto.randomUUID();
    const answer = await new Promise<ForeignDropAnswer | null>((resolve) => {
      this.foreignRequests.set(requestId, resolve);
      const { x, y } = this.toContent(drop.windowId, drop.point);
      this.windows.send(drop.windowId, IPC.foreignDrop, { requestId, x, y, candidates });
    });
    const chosen = drop.sessions.filter((s) => answer?.pids.includes(s.pid));
    if (chosen.length === 0) return this.log('info', 'terminal window takeover cancelled');
    this.log('info', 'terminal window takeover confirmed', {
      sessions: chosen.length,
      target: answer?.target ?? 'end of the window',
    });

    let target = answer?.target ?? null;
    const shells: (number | undefined)[] = [];
    for (const session of chosen) {
      // Remember a shell in that console, to ask it afterwards what is left there.
      const before = await consoleHelper({ type: 'console-processes', pid: session.pid });
      shells.push(before.ok ? before.processes?.find((p) => isShell(p.name))?.pid : undefined);
      this.log(
        'info',
        'running in that terminal tab',
        before.ok ? { processes: before.processes?.map((p) => p.name) } : { error: before.error },
      );
      const latest = readRegistryEntry(this.claudeHome, session.pid) ?? session;
      const result = await stopExternalClaude(session.pid, this.log);
      this.log(result === 'failed' ? 'warn' : 'info', 'other terminal stopped', { pid: session.pid, result });
      if (result === 'failed') continue;
      const name = latest.nameSource === 'user' ? latest.name : undefined;
      const record = this.adopted(latest.sessionId, latest.cwd ?? session.cwd!, name);
      this.placeAndResume(drop.windowId, record, target);
      target = { key: tileKey(record.id), side: 'right' };
    }

    for (const shell of shells) {
      // Without a shell, the tab closed on its own together with claude.
      if (shell !== undefined) await this.closeIdleConsole(shell);
    }
  }

  /**
   * Closes a terminal tab (console) that has nothing but shells left, by typing "exit" as its user would. Only
   * that tab goes; other tabs of the same terminal window are never touched, and the last one takes the
   * window with it. Nested shells (e.g. bash started from PowerShell) get one "exit" each.
   */
  private async closeIdleConsole(shellPid: number): Promise<void> {
    for (let attempt = 0; attempt < 3; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 500));
      const left = await consoleHelper({ type: 'console-processes', pid: shellPid });
      if (!left.ok) return this.log('info', 'terminal tab closed'); // nothing to attach to any more
      if (!onlyShells(left.processes ?? [])) {
        return this.log('info', 'terminal tab stays open, something else runs there', {
          running: left.processes?.map((p) => p.name),
        });
      }
      // Any shell of the tab will do: they share the console input, and the one in front reads the "exit".
      const shell = left.processes?.find((p) => isShell(p.name) && !/^(conhost|openconsole)\.exe$/i.test(p.name ?? ''));
      if (!shell) return;
      this.log('info', 'only a shell is left in the terminal tab, typing exit', { shell: shell.name });
      await consoleHelper({ type: 'type', pid: shell.pid, text: 'exit\r' });
    }
  }

  /** Test hook (REFRAME_E2E): the console window or hosting terminal window of a process. */
  async terminalWindowOf(pid: number): Promise<string | null> {
    const result = await consoleHelper({ type: 'owners', pids: [pid] });
    const owner = result.ok ? result.owners?.[0] : undefined;
    return owner ? (owner.owner ?? owner.consoleWindow) : null;
  }

  /** Test hook (REFRAME_E2E): terminal window `hwnd` as if it had just appeared at a screen point (torn-out tab). */
  terminalWindowAppeared(hwnd: string, point?: { x: number; y: number }): void {
    this.foreign?.appeared(hwnd, point);
  }

  /** Test hook (REFRAME_E2E): the terminal window a simulated tab drag comes from. */
  tabDragSource(hwnd: string | null): void {
    if (this.foreign) this.foreign.tabDragSource = hwnd;
  }

  /** Test hook (REFRAME_E2E): what the clipboard holds as far as reframe asks (null: the real one). */
  fakeClipboard(contents: ClipboardContents | null): void {
    this.clipboardOverride = contents;
  }
  private clipboardOverride: ClipboardContents | null = null;

  /** Test hook (REFRAME_E2E): this copy checks for updates, as an installed one does (without checking). */
  fakeUpdateChecks(): void {
    this.updates.checks = true;
    for (const w of this.state.windows) this.push(w.id);
  }

  /**
   * A file dropped onto a tile that exists only in memory (an image dragged from a browser, say) is written to a
   * temporary folder, so the session can get a path for it like for any other file.
   */
  private saveDroppedFile(name: string, data: ArrayBuffer | Uint8Array): string | null {
    if (data.byteLength > 200 * 1024 * 1024) return null;
    // A plain file name: no folders, nothing Windows refuses in names.
    const cleaned = [...path.basename(name)].map((c) => (c < ' ' || '<>:"/\\|?*'.includes(c) ? '_' : c)).join('');
    const safe = cleaned.slice(0, 120) || 'dropped';
    const dir = path.join(os.tmpdir(), 'reframe-drops', crypto.randomUUID());
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, safe);
    fs.writeFileSync(file, data instanceof Uint8Array ? data : Buffer.from(data));
    this.log('info', 'dropped file saved for a session', { bytes: data.byteLength });
    return file;
  }

  /** Test hook (REFRAME_E2E): what releasing terminal window `hwnd` at a screen point does. */
  async dropTerminalWindow(hwnd: string, point: { x: number; y: number }): Promise<boolean> {
    const windowId = this.windows.windowAt(point);
    const sessions = this.foreign && windowId ? await this.foreign.sessionsIn(hwnd) : [];
    if (!windowId || sessions.length === 0) return false;
    void this.takeOverWindow({ hwnd, title: win32()?.title(hwnd) ?? '', windowId, point, sessions });
    return true;
  }

  // ---------- notifications and tray ----------

  private notify(sessionId: string, kind: NotifyKind): void {
    const win = this.windowOf(sessionId);
    const record = this.state.sessions[sessionId];
    if (!win || !record) return;
    const skip = (reason: string) => this.log('info', 'notification skipped', { session: sessionId, kind, reason });
    if (!Notification.isSupported() || process.env.REFRAME_DISABLE_NOTIFICATIONS) return skip('notifications off');
    if (this.windows.isAttentive(win.id) && !isCollapsed(win, sessionId)) return skip('window in focus, tile visible');
    const label = this.sessionView(record).label;
    const notification = new Notification({
      title: translate(this.locale, kind === 'done' ? 'notify.done' : 'notify.waiting', { label }),
      body: translate(this.locale, kind === 'done' ? 'notify.doneBody' : 'notify.waitingBody'),
      icon: this.assets.icon,
    });
    notification.on('click', () => {
      this.log('info', 'notification clicked', { session: sessionId });
      this.focusSession(sessionId);
    });
    notification.show();
    this.log('info', 'notification shown', { session: sessionId, kind });
  }

  private focusSession(sessionId: string): void {
    const win = this.windowOf(sessionId);
    if (!win) return;
    this.windows.show(win.id);
    for (const entry of win.dock) {
      if (entry.kind === 'tile' && entry.sessionId === sessionId) {
        this.mutateWindow(win.id, (w) => applyLayoutAction(w, { type: 'tile/expand', sessionId }));
      } else if (entry.kind === 'group' && leaves(entry.node).includes(sessionId)) {
        this.mutateWindow(win.id, (w) => applyLayoutAction(w, { type: 'group/expand', groupId: entry.node.id }));
      }
    }
    this.windows.send(win.id, IPC.focusTile, sessionId);
  }

  private t = (key: MessageKey, params?: Record<string, string | number>): string =>
    translate(this.locale, key, params);

  private createTray(): void {
    const t = this.t;
    const { refresh } = createTray(this.assets.trayIcon, this.assets.trayIcon2x, {
      labels: () => ({
        tooltip: t('tray.tooltip'),
        newSession: t('tray.newSession'),
        newWindow: t('tray.newWindow'),
        workspaces: t('tray.workspaces'),
        noWorkspaces: t('workspace.none'),
        autostart: t('tray.autostart'),
        about: t('tray.about'),
        quit: t('tray.quit'),
      }),
      windows: () =>
        this.state.windows.map((w, i) => {
          const labels = windowSessions(w)
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
      newWindow: () => this.newWindow(),
      workspaces: () => this.workspaceItems().map((w) => ({ id: w.id, label: workspaceLabel(t, w) })),
      openWorkspace: (id) => this.openWorkspace(id),
      about: () => {
        const id = this.state.windows[0]!.id;
        this.windows.show(id);
        this.windows.send(id, IPC.openAbout, null);
      },
      autostart: () => {
        try {
          return autostartEnabled();
        } catch {
          return false;
        }
      },
      setAutostart: (enabled) => {
        try {
          setAutostart(enabled);
          this.log('info', enabled ? 'starts with the system now' : 'no longer starts with the system');
        } catch (error) {
          this.log('warn', 'could not change starting with the system', { error: String(error) });
        }
      },
      update: () => {
        if (this.updates.ready) {
          return {
            label: t('tray.restartToUpdate', { version: this.updates.ready }),
            install: () => this.restartToUpdate(),
          };
        }
        // The portable copy: the new version is downloaded by hand.
        if (this.updates.available) {
          return {
            label: t('tray.downloadUpdate', { version: this.updates.available }),
            install: () => void shell.openExternal(RELEASES_URL),
          };
        }
        return null;
      },
      quit: () => app.quit(),
    });
    this.refreshTray = refresh;
  }

  private watchUpdates(): void {
    this.updates.on('ready', (version) => {
      this.refreshTray();
      if (!Notification.isSupported() || process.env.REFRAME_DISABLE_NOTIFICATIONS) return;
      new Notification({
        title: this.t('update.readyTitle'),
        body: this.t('update.readyBody', { version }),
        icon: this.assets.icon,
      }).show();
    });
    this.updates.on('available', (version) => {
      this.refreshTray();
      if (!Notification.isSupported() || process.env.REFRAME_DISABLE_NOTIFICATIONS) return;
      const notification = new Notification({
        title: this.t('update.availableTitle', { version }),
        body: this.t('update.availableBody'),
        icon: this.assets.icon,
      });
      notification.on('click', () => void shell.openExternal(RELEASES_URL));
      notification.show();
    });
    this.updates.start();
  }

  /** Ends the sessions as on quit (they resume after the restart), then installs the update. */
  private async restartToUpdate(): Promise<void> {
    this.log('info', 'restarting to install an update', { version: this.updates.ready });
    await this.shutdown();
    this.updates.install();
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
    ipcMain.on(IPC.transferNode, (event, request: Partial<TransferRequest> | null) => {
      const id = windowId(event);
      if (!id || !request || typeof request.key !== 'string' || typeof request.fromWindowId !== 'string') return;
      if (request.target !== null && !isDropTarget(request.target)) return;
      this.transferNode(id, { key: request.key, fromWindowId: request.fromWindowId, target: request.target });
    });
    ipcMain.on(IPC.newWindow, (event) => this.newWindow(windowId(event)));
    ipcMain.on(IPC.toggleFullscreen, (event) => {
      const id = windowId(event);
      if (id) this.windows.toggleFullscreen(id);
    });
    ipcMain.handle(IPC.readClipboard, () => this.clipboardOverride ?? readClipboard());
    ipcMain.handle(IPC.saveDroppedFile, (_event, name: unknown, data: unknown) =>
      typeof name === 'string' && data instanceof ArrayBuffer ? this.saveDroppedFile(name, data) : null,
    );
    ipcMain.on(IPC.tearOut, (event, key: unknown, size: Partial<TileSize> | null) => {
      const id = windowId(event);
      if (!id || typeof key !== 'string') return;
      const valid = !!size && Number.isFinite(size.width) && Number.isFinite(size.height);
      this.tearOut(id, key, screen.getCursorScreenPoint(), valid ? (size as TileSize) : undefined);
    });
    ipcMain.on(IPC.tileMenu, (event, sessionId: unknown) => {
      const id = windowId(event);
      if (id && typeof sessionId === 'string') this.showTileMenu(id, sessionId);
    });
    ipcMain.on(IPC.renameSession, (_event, sessionId: unknown, name: unknown) => {
      if (typeof sessionId === 'string' && typeof name === 'string') this.renameSession(sessionId, name);
    });
    ipcMain.on(IPC.workspaceMenu, (event) => {
      const id = windowId(event);
      if (id) this.showWorkspaceMenu(id);
    });
    ipcMain.on(IPC.workspaceItemMenu, (event, workspaceId: unknown) => {
      const id = windowId(event);
      if (id && typeof workspaceId === 'string') this.showWorkspaceItemMenu(id, workspaceId);
    });
    ipcMain.on(IPC.saveWorkspace, (event, name: unknown) => {
      const id = windowId(event);
      if (id && typeof name === 'string') this.saveWorkspace(id, name);
    });
    ipcMain.on(IPC.openWorkspace, (_event, workspaceId: unknown) => {
      if (typeof workspaceId === 'string') this.openWorkspace(workspaceId);
    });
    ipcMain.on(IPC.renameWorkspace, (_event, workspaceId: unknown, name: unknown) => {
      if (typeof workspaceId === 'string' && typeof name === 'string') this.renameWorkspace(workspaceId, name);
    });
    ipcMain.on(IPC.listMenu, (event, items: unknown) => {
      const id = windowId(event);
      if (id && isListItems(items)) this.showListMenu(id, items);
    });
    ipcMain.on(IPC.openSessions, (event, request: Partial<OpenSessionsRequest> | null) => {
      const id = windowId(event);
      if (!id || !isListItems(request?.items) || !OPEN_MODES.includes(request.mode as OpenMode)) return;
      if (request.target !== null && !isDropTarget(request.target)) return;
      void this.openSessions(id, request.items, request.mode as OpenMode, request.target ?? null).catch(
        (error: unknown) => this.log('error', 'opening sessions from the list failed', { error: String(error) }),
      );
    });
    ipcMain.on(IPC.groupMenu, (event, groupId: unknown) => {
      const id = windowId(event);
      if (id && typeof groupId === 'string') this.showGroupMenu(id, groupId, false);
    });
    ipcMain.on(IPC.groupColorMenu, (event, groupId: unknown) => {
      const id = windowId(event);
      if (id && typeof groupId === 'string') this.showGroupMenu(id, groupId, true);
    });
    ipcMain.on(IPC.openExternal, (_event, url: unknown) => {
      if (typeof url === 'string' && /^https?:\/\//i.test(url)) void shell.openExternal(url);
    });
    ipcMain.handle(IPC.dialogDefaults, () => ({
      recentFolders: this.state.recentFolders.filter((folder) => !!this.wslPath(folder) || isDirectory(folder)),
      preferences: this.state.preferences,
    }));
    ipcMain.on(IPC.setPreferences, (_event, patch: Partial<Preferences> | null) => {
      if (!patch || typeof patch !== 'object') return;
      const next = { ...this.state.preferences };
      if (typeof patch.nameFromFolder === 'boolean') next.nameFromFolder = patch.nameFromFolder;
      if (LANGUAGES.includes(patch.language as Language)) next.language = patch.language as Language;
      const size = patch.fontSize;
      if (Number.isInteger(size) && size! >= FONT_SIZE.min && size! <= FONT_SIZE.max) next.fontSize = size!;
      if (typeof patch.snapToGrid === 'boolean') next.snapToGrid = patch.snapToGrid;
      if (typeof patch.checkForUpdates === 'boolean') next.checkForUpdates = patch.checkForUpdates;
      if (CLOSE_WINDOW.includes(patch.closeWindow as CloseWindow)) next.closeWindow = patch.closeWindow as CloseWindow;
      const relabel = next.language !== this.state.preferences.language;
      const was = this.state.preferences;
      const shown =
        next.fontSize !== was.fontSize ||
        next.snapToGrid !== was.snapToGrid ||
        next.checkForUpdates !== was.checkForUpdates ||
        next.closeWindow !== was.closeWindow;
      this.state = { ...this.state, preferences: next };
      this.save();
      if (relabel) {
        // Every window, the tray and whatever comes next (menus, dialogs, notifications) speak it right away.
        this.applyLanguage();
        this.log('info', 'language changed', { language: next.language, locale: this.locale });
        this.refreshTray();
      }
      if (relabel || shown) for (const w of this.state.windows) this.push(w.id);
    });
    ipcMain.handle(IPC.folderExists, (_event, folder: unknown) => typeof folder === 'string' && isFolder(folder));
    ipcMain.on(IPC.startWsl, (_event, distro: unknown) => {
      if (typeof distro === 'string' && (this.state.wslDistros ?? []).includes(distro)) this.wsl?.keep(distro);
    });
    ipcMain.handle(IPC.saveClipboardImage, async () => {
      const png = this.clipboardOverride ? (this.clipboardOverride.image ? TEST_PNG : null) : await clipboardImage();
      return png && this.saveDroppedFile('clipboard.png', png);
    });
    ipcMain.handle(IPC.sessionList, (_event, query: unknown, limit: unknown) =>
      this.sessionList(typeof query === 'string' ? query : '', typeof limit === 'number' ? Math.min(limit, 500) : 20),
    );
    ipcMain.on(IPC.openSession, (event, request: Partial<OpenSessionRequest> | null) => {
      const id = windowId(event);
      const item = request?.item;
      if (!id || !item || !isListItemRef(item)) return;
      if (request.target !== null && !isDropTarget(request.target)) return;
      void this.openSession(id, item, request.target ?? null).catch((error: unknown) =>
        this.log('error', 'opening a session from the list failed', { error: String(error) }),
      );
    });
    ipcMain.on(IPC.foreignPointer, (event, report: Partial<ForeignPointerReport> | null) => {
      const id = windowId(event);
      if (!id) return;
      if (typeof report?.x !== 'number' || typeof report.y !== 'number') return this.foreign?.notePointer(id, null, []);
      const types = Array.isArray(report.types) ? report.types.filter((t) => typeof t === 'string') : [];
      this.foreign?.notePointer(id, { x: report.x, y: report.y }, types);
    });
    ipcMain.on(IPC.foreignDropAnswer, (_event, requestId: unknown, answer: Partial<ForeignDropAnswer> | null) => {
      if (typeof requestId !== 'string') return;
      const resolve = this.foreignRequests.get(requestId);
      this.foreignRequests.delete(requestId);
      const valid =
        !!answer &&
        Array.isArray(answer.pids) &&
        answer.pids.every((p) => typeof p === 'number') &&
        (answer.target === null || isDropTarget(answer.target));
      resolve?.(valid ? { pids: answer.pids!, target: answer.target ?? null } : null);
    });
    ipcMain.on(IPC.removeSession, (_event, sessionId: unknown) => {
      if (typeof sessionId === 'string') this.removeSession(sessionId);
    });
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

const OPEN_MODES: OpenMode[] = ['open', 'group', 'groupInNewWindow'];

function isListItems(value: unknown): value is ListItemRef[] {
  return Array.isArray(value) && value.length > 0 && value.length <= 200 && value.every(isListItemRef);
}

function isListItemRef(value: unknown): value is ListItemRef {
  if (!value || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  if (v.kind === 'app') return typeof v.sessionId === 'string';
  if (v.distro !== undefined && typeof v.distro !== 'string') return false;
  if (v.kind === 'found') return typeof v.claudeSessionId === 'string';
  if (v.kind === 'running') return typeof v.pid === 'number' && typeof v.claudeSessionId === 'string';
  return false;
}

/** Whether a folder is there, without holding up the app (a WSL folder may wait for its distribution to start). */
function isFolder(folder: string): Promise<boolean> {
  return fs.promises.stat(folder).then(
    (stats) => stats.isDirectory(),
    () => false,
  );
}

/** A 1×1 PNG, for the clipboard image the tests fake. */
const TEST_PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==',
  'base64',
);

function isDirectory(folder: string): boolean {
  try {
    return fs.statSync(folder).isDirectory();
  } catch {
    return false;
  }
}
