// Contract between main and renderer windows.
import type { LayoutAction } from './actions';
import type { DropTarget, LayoutNode } from './layout';
import type { Locale } from './i18n';
import type { PermissionMode } from './launch';
import type { DockEntry, SessionRecord } from './state';

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
  getRecentFolders(): Promise<string[]>;
  folderExists(path: string): Promise<boolean>;
  onFocusTile(listener: (sessionId: string) => void): () => void;
  /** The tray asks the window to open the "New session" dialog. */
  onOpenNewSession(listener: () => void): () => void;
  /** Opens an http(s) link from a terminal in the default browser. */
  openExternal(url: string): void;
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
  recentFolders: 'reframe:recent-folders',
  folderExists: 'reframe:folder-exists',
  focusTile: 'reframe:focus-tile',
  openNewSession: 'reframe:open-new-session',
  openExternal: 'reframe:open-external',
} as const;

/** window.postMessage payload carrying a terminal MessagePort from preload to the page. */
export const TERMINAL_PORT_MESSAGE = 'reframe:terminal-port';

/** Messages on a terminal MessagePort. */
export type TerminalHostMessage =
  | { type: 'snapshot'; data: string; cols: number; rows: number }
  | { type: 'data'; data: string }
  | { type: 'exit'; exitCode: number };

export type TerminalViewMessage = { type: 'input'; data: string } | { type: 'resize'; cols: number; rows: number };

/** MIME type used for dragging tiles (HTML5 DnD). */
export const TILE_MIME = 'application/x-reframe-tile';
