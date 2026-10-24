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
import { emptyWindow, type AppState, type Preferences, type SessionRecord, type WindowState } from '@shared/state';
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
import { colorMenu, groupMenu, listMenu, tileMenu } from './menus';
import { createTray } from './tray';
import { readClipboard } from './clipboard';
import { Updates } from './updates';
import { autostartEnabled, setAutostart } from './autostart';
import { win32 } from './win32';
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
  private log = createLogger(path.join(app.getPath('userData'), 'logs'));
  private claudeHome = claudeHome();
  private transcripts = new TranscriptIndex(this.claudeHome);
  private updates = new Updates(this.log);

  constructor(
    private locale: Locale,
    private assets: AppAssets,
  ) {
    this.windows = new WindowManager(assets.icon);
  }

  /** `hidden`: started by the system, so the windows stay in the tray (sessions resume all the same). */
  async start({ hidden = false }: { hidden?: boolean } = {}): Promise<void> {
    const loaded = this.store.load();
    this.state = loaded.state;
    if (this.state.windows.length === 0) this.state.windows.push(emptyWindow(crypto.randomUUID()));
    this.log('info', 'reframe starting', {
      version: app.getVersion(),
      platform: process.platform,
      build: app.isPackaged ? 'installed' : 'development',
      hidden,
      state: loaded.source,
      windows: this.state.windows.length,
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
    });
    this.sessions.on('changed', (id) => this.pushSession(id));
    this.sessions.on('identity', (id, patch) => this.updateIdentity(id, patch));
    this.sessions.on('notify', (id, kind) => this.notify(id, kind));
    this.sessions.on('ended', (id) => this.updateSession(id, { lifecycle: 'ended' }));
    await this.sessions.cleanupOrphans(Object.values(this.state.sessions));

    this.registerIpc();
    this.openWindows(hidden);
    this.watchForeignWindows();
    this.resumeSessions();
    this.createTray();
    this.watchUpdates();
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
    this.log('info', 'reframe quitting, ending sessions');
    this.save();
    await this.sessions.stopAll();
    this.sessions.dispose();
    this.host.stop();
    this.hooks.stop();
    this.save();
    this.log('info', 'reframe stopped');
  }

  // ---------- windows and views ----------

  private openWindows(hidden: boolean): void {
    const anyVisible = this.state.windows.some((w) => !w.hidden);
    this.state.windows.forEach((w, i) => this.windows.open(w, !hidden && (!w.hidden || (!anyVisible && i === 0))));
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
    for (const id of windowSessions(win)) {
      const record = this.state.sessions[id];
      if (record) sessions[id] = this.sessionView(record);
    }
    return {
      windowId,
      locale: this.locale,
      sidebarOpen: !!win.sidebarOpen,
      layout: win.layout,
      dock: win.dock,
      sessions,
    };
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
    const menu = listMenu(this.t, {
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

    const ownIds = new Set(records.map((r) => r.claudeSessionId));
    const running: RunningListItem[] = this.externalRunning()
      .filter((entry) => matches(entry.name, entry.cwd))
      .map((entry) => ({
        kind: 'running',
        pid: entry.pid,
        claudeSessionId: entry.sessionId,
        cwd: entry.cwd!,
        name: entry.name,
      }));

    const exclude = new Set([...ownIds, ...running.map((r) => r.claudeSessionId)]);
    const past = await this.transcripts.list({ query, limit, exclude });
    const found = past.items
      .filter((info) => info.cwd)
      .map((info) => ({
        kind: 'found' as const,
        claudeSessionId: info.sessionId,
        cwd: info.cwd!,
        title: info.title,
        modifiedAt: info.modifiedAt,
      }));
    return { app, running, found, hasMore: past.hasMore };
  }

  /** Claudes running outside reframe (with a known folder). */
  private externalRunning(): RegistryEntry[] {
    const records = Object.values(this.state.sessions);
    const ownPids = new Set(records.map((r) => this.sessions.status(r.id)?.pid).filter((p): p is number => !!p));
    const ownIds = new Set(records.map((r) => r.claudeSessionId));
    return listRunning(this.claudeHome, { pids: ownPids, sessionIds: ownIds }).filter((entry) => entry.cwd);
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

    if (item.kind === 'found') {
      const info = await this.transcripts.find(item.claudeSessionId);
      if (info?.cwd) return this.adopted(item.claudeSessionId, info.cwd);
      this.log('warn', 'found session has no folder', { claudeSession: item.claudeSessionId });
      return null;
    }

    const entry = readRegistryEntry(this.claudeHome, item.pid);
    if (!entry || entry.sessionId !== item.claudeSessionId || !entry.cwd || !isAlive(item.pid)) {
      this.log('info', 'running session is gone, nothing to take over', { pid: item.pid });
      return null;
    }
    if (!confirmed && !(await this.confirmTakeover(windowId, [entry.name ?? path.basename(entry.cwd)]))) return null;
    this.log('info', 'taking over session from another terminal', { pid: item.pid, claudeSession: entry.sessionId });
    const result = await stopExternalClaude(item.pid, this.log);
    this.log(result === 'failed' ? 'warn' : 'info', 'other terminal stopped', { pid: item.pid, result });
    if (result === 'failed') {
      await this.showError(windowId, this.t('takeover.failed'));
      return null;
    }
    // A name the user gave it survives; one Claude made up is made up again.
    const name = entry.nameSource === 'user' ? entry.name : undefined;
    return this.adopted(entry.sessionId, entry.cwd, name);
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
      const labels = running.map((i) => {
        const entry = readRegistryEntry(this.claudeHome, i.pid);
        return entry?.name ?? (entry?.cwd ? path.basename(entry.cwd) : String(i.pid));
      });
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
      if (!this.sessions.isRunning(id)) this.sessions.start(this.state.sessions[id]!, 'resume');
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
  private adopted(claudeSessionId: string, cwd: string, name?: string): SessionRecord {
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
      used: transcriptExists(this.claudeHome, claudeSessionId),
    };
  }

  private placeAndResume(windowId: string, record: SessionRecord, target: DropTarget | null): void {
    this.state = { ...this.state, sessions: { ...this.state.sessions, [record.id]: record } };
    this.mutateWindow(windowId, (w) => putNode(w, tile(record.id), target));
    this.log('info', 'session opened from the list', { session: record.id, window: windowId });
    this.sessions.start(record, 'resume');
    this.windows.send(windowId, IPC.focusTile, record.id);
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
      externalSessions: () => this.externalRunning(),
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

  /**
   * A file dropped onto a tile that exists only in memory (an image dragged from a browser, say) is written to a
   * temporary folder, so the session can get a path for it like for any other file.
   */
  private saveDroppedFile(name: string, data: ArrayBuffer): string | null {
    if (data.byteLength > 200 * 1024 * 1024) return null;
    // A plain file name: no folders, nothing Windows refuses in names.
    const cleaned = [...path.basename(name)].map((c) => (c < ' ' || '<>:"/\\|?*'.includes(c) ? '_' : c)).join('');
    const safe = cleaned.slice(0, 120) || 'dropped';
    const dir = path.join(os.tmpdir(), 'reframe-drops', crypto.randomUUID());
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, safe);
    fs.writeFileSync(file, Buffer.from(data));
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
      tooltip: t('tray.tooltip'),
      newSessionLabel: t('tray.newSession'),
      quitLabel: t('tray.quit'),
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
      newWindowLabel: t('tray.newWindow'),
      newWindow: () => this.newWindow(),
      autostartLabel: t('tray.autostart'),
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
      update: () =>
        this.updates.ready
          ? { label: t('tray.restartToUpdate', { version: this.updates.ready }), install: () => this.restartToUpdate() }
          : null,
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
      recentFolders: this.state.recentFolders.filter(isDirectory),
      preferences: this.state.preferences,
    }));
    ipcMain.on(IPC.setPreferences, (_event, patch: Partial<Preferences> | null) => {
      if (!patch || typeof patch !== 'object') return;
      const next = { ...this.state.preferences };
      if (typeof patch.nameFromFolder === 'boolean') next.nameFromFolder = patch.nameFromFolder;
      this.state = { ...this.state, preferences: next };
      this.save();
    });
    ipcMain.handle(IPC.folderExists, (_event, folder: unknown) => typeof folder === 'string' && isDirectory(folder));
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
  if (v.kind === 'found') return typeof v.claudeSessionId === 'string';
  if (v.kind === 'running') return typeof v.pid === 'number' && typeof v.claudeSessionId === 'string';
  return false;
}

function isDirectory(folder: string): boolean {
  try {
    return fs.statSync(folder).isDirectory();
  } catch {
    return false;
  }
}
