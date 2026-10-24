// Contract between main and renderer windows.
import type { LayoutAction } from './actions';
import type { DropTarget, LayoutNode } from './layout';
import type { Locale } from './i18n';
import type { PermissionMode } from './launch';
import type { DockEntry, Preferences, SessionRecord } from './state';

/** starting → idle/busy/waiting while running; ended once the process is gone; error if it could not run. */
export type SessionStatus = 'starting' | 'idle' | 'busy' | 'waiting' | 'ended' | 'error';

export interface SessionView {
  id: string;
  /** What the tile shows: the user's name, else Claude's name for the session, else the folder. */
  label: string;
  cwd: string;
  status: SessionStatus;
  lifecycle: SessionRecord['lifecycle'];
  /** Translation key of the error shown in the tile, if any. */
  error?: 'resumeFailed' | 'transcriptMissing' | 'claudeNotFound' | 'spawnFailed' | 'folderMissing';
}

export interface WindowView {
  windowId: string;
  locale: Locale;
  sidebarOpen: boolean;
  layout: LayoutNode | null;
  dock: DockEntry[];
  sessions: Record<string, SessionView>;
}

export interface NewSessionRequest {
  cwd: string;
  name?: string;
  worktree?: boolean;
  worktreeName?: string;
  model?: string;
  permissionMode?: PermissionMode;
  target?: DropTarget;
}

export type NewSessionResult = { ok: true } | { ok: false; error: 'folderMissing' | 'claudeNotFound' };

export interface DialogDefaults {
  recentFolders: string[];
  preferences: Preferences;
}

/**
 * A tile or group (by node key) dropped into this window from another one; without a target it is added
 * at the end (or fills an empty window).
 */
export interface TransferRequest {
  key: string;
  fromWindowId: string;
  target: DropTarget | null;
}

/** A session reframe manages, for the session list. */
export interface AppListItem {
  kind: 'app';
  id: string;
  label: string;
  cwd: string;
  status: SessionStatus;
  lifecycle: SessionRecord['lifecycle'];
  /** Window that shows it (1-based number for display), or null when it is in no window. */
  windowId: string | null;
  windowNumber: number | null;
  lastActiveAt: number;
}

/** A claude running outside reframe (from Claude Code's session registry). */
export interface RunningListItem {
  kind: 'running';
  pid: number;
  claudeSessionId: string;
  cwd: string;
  name?: string;
}

/** A past session found in Claude Code's transcripts. */
export interface FoundListItem {
  kind: 'found';
  claudeSessionId: string;
  cwd: string;
  title?: string;
  modifiedAt: number;
}

export interface SessionList {
  app: AppListItem[];
  running: RunningListItem[];
  found: FoundListItem[];
  /** More past sessions exist beyond the requested limit. */
  hasMore: boolean;
}

/** What a list entry stands for when it is opened (main looks the details up again itself). */
export type ListItemRef =
  | { kind: 'app'; sessionId: string }
  | { kind: 'running'; pid: number; claudeSessionId: string }
  | { kind: 'found'; claudeSessionId: string };

export interface OpenSessionRequest {
  item: ListItemRef;
  target: DropTarget | null;
}

/** A claude found in a terminal window that was dropped onto reframe. */
export interface ForeignCandidate {
  pid: number;
  claudeSessionId: string;
  cwd: string;
  name?: string;
  /** The one the terminal window shows right now (or the only one). */
  preselected: boolean;
}

/** A terminal window was dropped at (x, y) in this window's content; asks which sessions to take over. */
export interface ForeignDropRequest {
  requestId: string;
  x: number;
  y: number;
  candidates: ForeignCandidate[];
}

/** The clipboard as far as pasting into a session goes. */
export interface ClipboardContents {
  text: string;
  image: boolean;
  /** Files copied in a file manager. */
  files: string[];
}

export interface ForeignPointerReport {
  x: number;
  y: number;
  /** Data types the drag offers (a Windows Terminal tab offers none). */
  types: string[];
}

export interface ForeignDropAnswer {
  pids: number[];
  target: DropTarget | null;
}

/** Size of the tile or group when the drag started, so a torn-out window can match it. */
export interface TileSize {
  width: number;
  height: number;
}

/** API exposed to renderers as `window.reframe`. */
export interface ReframeApi {
  getView(): Promise<WindowView>;
  onView(listener: (view: WindowView) => void): () => void;
  dispatch(action: LayoutAction): void;
  createSession(request: NewSessionRequest): Promise<NewSessionResult>;
  /** Restarts an ended or failed session in place (resume, or a fresh start when `fresh`). */
  restartSession(sessionId: string, fresh?: boolean): void;
  /** Ends the session and removes its tile; it stays in the session list. */
  endSession(sessionId: string): void;
  /** Asks main to send a MessagePort for the session's terminal (arrives as a window message). */
  attachTerminal(sessionId: string): void;
  pickFolder(): Promise<string | null>;
  /** Recent folders and saved preferences for the "New session" dialog. */
  getDialogDefaults(): Promise<DialogDefaults>;
  setPreferences(patch: Partial<Preferences>): void;
  folderExists(path: string): Promise<boolean>;
  onFocusTile(listener: (sessionId: string) => void): () => void;
  /** The tray asks the window to open the "New session" dialog. */
  onOpenNewSession(listener: () => void): () => void;
  /** Opens an http(s) link from a terminal in the default browser. */
  openExternal(url: string): void;
  /** Opens a new, empty window next to this one. */
  newWindow(): void;
  /** `process.platform`, e.g. for how to quote paths. */
  readonly platform: string;
  readClipboard(): Promise<ClipboardContents>;
  /** The path of a dropped file on disk; "" for one that only exists in memory (e.g. an image from a browser). */
  pathForFile(file: File): string;
  /** Saves a dropped file that has no path to a temporary file and returns its path. */
  saveDroppedFile(name: string, data: ArrayBuffer): Promise<string | null>;
  /** Moves a tile or group from another window into this one. */
  transferNode(request: TransferRequest): void;
  /** A drag ended without a drop target; if the pointer is outside every window, the node gets a new window. */
  tearOut(key: string, size: TileSize): void;
  /** Native context menu for a tile (grouping). */
  showTileMenu(sessionId: string): void;
  /** Native menu for a group (rename, color, dissolve, end all). */
  showGroupMenu(groupId: string): void;
  /** Native menu with just the group colors. */
  showGroupColorMenu(groupId: string): void;
  /** The group menu's "Rename" asks the window to edit the group's name in place. */
  onRenameGroup(listener: (groupId: string) => void): () => void;
  getSessionList(query: string, limit: number): Promise<SessionList>;
  /** Shows a session that is open somewhere, or opens one from the list in this window (resuming it). */
  openSession(request: OpenSessionRequest): void;
  /** Forgets an ended session (its Claude Code transcript stays). */
  removeSession(sessionId: string): void;
  /** A terminal window with claude is dragged over this window (content coordinates); null when it left. */
  onForeignDrag(listener: (point: { x: number; y: number } | null) => void): () => void;
  /** Such a terminal window was dropped here: confirm or pick the sessions to take over. */
  onForeignDrop(listener: (request: ForeignDropRequest) => void): () => void;
  /** Answer to a foreign drop; null cancels. */
  answerForeignDrop(requestId: string, answer: ForeignDropAnswer | null): void;
  /** Something unknown (not ours, e.g. a terminal tab) is dragged over this window: pointer in screen
   * coordinates; null when it left the window. */
  reportForeignPointer(report: ForeignPointerReport | null): void;
}

export const IPC = {
  getView: 'reframe:get-view',
  view: 'reframe:view',
  dispatch: 'reframe:dispatch',
  createSession: 'reframe:create-session',
  restartSession: 'reframe:restart-session',
  endSession: 'reframe:end-session',
  attachTerminal: 'reframe:attach-terminal',
  terminalPort: 'reframe:terminal-port',
  pickFolder: 'reframe:pick-folder',
  dialogDefaults: 'reframe:dialog-defaults',
  setPreferences: 'reframe:set-preferences',
  folderExists: 'reframe:folder-exists',
  focusTile: 'reframe:focus-tile',
  openNewSession: 'reframe:open-new-session',
  openExternal: 'reframe:open-external',
  transferNode: 'reframe:transfer-node',
  newWindow: 'reframe:new-window',
  readClipboard: 'reframe:read-clipboard',
  saveDroppedFile: 'reframe:save-dropped-file',
  tearOut: 'reframe:tear-out',
  tileMenu: 'reframe:tile-menu',
  groupMenu: 'reframe:group-menu',
  groupColorMenu: 'reframe:group-color-menu',
  renameGroup: 'reframe:rename-group',
  sessionList: 'reframe:session-list',
  openSession: 'reframe:open-session',
  removeSession: 'reframe:remove-session',
  foreignDrag: 'reframe:foreign-drag',
  foreignDrop: 'reframe:foreign-drop',
  foreignDropAnswer: 'reframe:foreign-drop-answer',
  foreignPointer: 'reframe:foreign-pointer',
} as const;

/** window.postMessage payload carrying a terminal MessagePort from preload to the page. */
export const TERMINAL_PORT_MESSAGE = 'reframe:terminal-port';

/** Messages on a terminal MessagePort. */
export type TerminalHostMessage =
  | { type: 'snapshot'; data: string; cols: number; rows: number }
  | { type: 'data'; data: string }
  | { type: 'exit'; exitCode: number };

export type TerminalViewMessage = { type: 'input'; data: string } | { type: 'resize'; cols: number; rows: number };

/** MIME types used for dragging (HTML5 DnD); the type tells tiles from groups before the drop. */
export const TILE_MIME = 'application/x-reframe-tile';
export const GROUP_MIME = 'application/x-reframe-group';
/** A session dragged out of the session list (payload: ListItemRef). */
export const LIST_MIME = 'application/x-reframe-list-item';
