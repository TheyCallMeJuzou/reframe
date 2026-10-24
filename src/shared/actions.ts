// Layout intents a window can send, and the pure reducer main applies to its WindowState.
import { insertTile, leaves, moveTile, removeTile, restoreNode, setSplitSizes, tile, type DropTarget } from './layout';
import type { WindowState } from './state';

export type LayoutAction =
  | { type: 'tile/move'; sessionId: string; target: DropTarget }
  | { type: 'tile/collapse'; sessionId: string }
  | { type: 'tile/expand'; sessionId: string }
  | { type: 'split/resize'; splitId: string; sizes: number[] };

export function windowHasSession(win: WindowState, sessionId: string): boolean {
  return leaves(win.layout).includes(sessionId) || win.dock.some((d) => d.sessionId === sessionId);
}

export function applyLayoutAction(win: WindowState, action: LayoutAction): WindowState {
  switch (action.type) {
    case 'tile/move':
      return { ...win, layout: moveTile(win.layout, action.sessionId, action.target) };
    case 'tile/collapse': {
      const { root, removed, hint } = removeTile(win.layout, action.sessionId);
      if (!removed) return win;
      return { ...win, layout: root, dock: [...win.dock, { kind: 'tile', sessionId: action.sessionId, hint }] };
    }
    case 'tile/expand': {
      const entry = win.dock.find((d) => d.sessionId === action.sessionId);
      if (!entry) return win;
      return {
        ...win,
        layout: restoreNode(win.layout, tile(action.sessionId), entry.hint),
        dock: win.dock.filter((d) => d !== entry),
      };
    }
    case 'split/resize':
      return { ...win, layout: setSplitSizes(win.layout, action.splitId, action.sizes) };
  }
}

/** Adds a new session's tile; with a target it lands next to that node, otherwise at the end. */
export function addTile(win: WindowState, sessionId: string, target?: DropTarget): WindowState {
  return { ...win, layout: insertTile(win.layout, sessionId, target) };
}

/** Removes a session from the window entirely (layout and dock). */
export function dropSession(win: WindowState, sessionId: string): WindowState {
  return {
    ...win,
    layout: removeTile(win.layout, sessionId).root,
    dock: win.dock.filter((d) => d.sessionId !== sessionId),
  };
}

/** A window with neither tiles nor collapsed tiles. */
export function isWindowEmpty(win: WindowState): boolean {
  return win.layout === null && win.dock.length === 0;
}

export function isDropTarget(value: unknown): value is DropTarget {
  if (!value || typeof value !== 'object') return false;
  const t = value as Record<string, unknown>;
  return typeof t.key === 'string' && ['left', 'right', 'top', 'bottom'].includes(t.side as string);
}

export function isLayoutAction(value: unknown): value is LayoutAction {
  if (!value || typeof value !== 'object') return false;
  const a = value as Record<string, unknown>;
  switch (a.type) {
    case 'tile/move':
      return typeof a.sessionId === 'string' && isDropTarget(a.target);
    case 'tile/collapse':
    case 'tile/expand':
      return typeof a.sessionId === 'string';
    case 'split/resize':
      return typeof a.splitId === 'string' && Array.isArray(a.sizes) && a.sizes.every((s) => typeof s === 'number');
    default:
      return false;
  }
}
