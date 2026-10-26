// Layout intents a window can send, and the pure reducer main applies to its WindowState.
import {
  appendNode,
  findNode,
  groupOf,
  groups,
  insertNode,
  insertTile,
  leaves,
  moveNode,
  removeNode,
  removeTile,
  resolveTarget,
  restoreNode,
  setSplitSizes,
  tile,
  tileKey,
  updateGroup,
  type DropTarget,
  type GroupNode,
  type LayoutNode,
} from './layout';
import type { DockEntry, WindowState } from './state';

export type LayoutAction =
  /** Moves a tile or a whole group (by node key) next to a target in the same window. */
  | { type: 'node/move'; key: string; target: DropTarget }
  | { type: 'tile/collapse'; sessionId: string }
  | { type: 'tile/expand'; sessionId: string }
  | { type: 'group/collapse'; groupId: string }
  | { type: 'group/expand'; groupId: string }
  | { type: 'group/rename'; groupId: string; name: string }
  | { type: 'split/resize'; splitId: string; sizes: number[] }
  | { type: 'window/sidebar'; open: boolean };

export function dockSessions(entry: DockEntry): string[] {
  return entry.kind === 'tile' ? [entry.sessionId] : leaves(entry.node);
}

/** Every session in the window, visible or collapsed. */
export function windowSessions(win: WindowState): string[] {
  return [...leaves(win.layout), ...win.dock.flatMap(dockSessions)];
}

export function windowHasSession(win: WindowState, sessionId: string): boolean {
  return windowSessions(win).includes(sessionId);
}

/** Whether the session is collapsed into the side bar (alone or with its group). */
export function isCollapsed(win: WindowState, sessionId: string): boolean {
  return win.dock.some((d) => dockSessions(d).includes(sessionId));
}

export function applyLayoutAction(win: WindowState, action: LayoutAction): WindowState {
  switch (action.type) {
    case 'node/move':
      return { ...win, layout: moveNode(win.layout, action.key, action.target) };
    case 'tile/collapse': {
      // The last tile of a group takes its group along, otherwise the group would be lost.
      const group = groupOf(win.layout, tileKey(action.sessionId));
      if (group && leaves(group).length === 1) {
        return applyLayoutAction(win, { type: 'group/collapse', groupId: group.id });
      }
      const { root, removed, hint } = removeTile(win.layout, action.sessionId);
      if (!removed) return win;
      return { ...win, layout: root, dock: [...win.dock, { kind: 'tile', sessionId: action.sessionId, hint }] };
    }
    case 'tile/expand': {
      const entry = win.dock.find((d) => d.kind === 'tile' && d.sessionId === action.sessionId);
      if (!entry) return win;
      return {
        ...win,
        layout: restoreNode(win.layout, tile(action.sessionId), entry.hint),
        dock: win.dock.filter((d) => d !== entry),
      };
    }
    case 'group/collapse': {
      const { root, removed, hint } = removeNode(win.layout, action.groupId);
      if (!removed || removed.kind !== 'group') return win;
      return { ...win, layout: root, dock: [...win.dock, { kind: 'group', node: removed, hint }] };
    }
    case 'group/expand': {
      const entry = win.dock.find((d) => d.kind === 'group' && d.node.id === action.groupId);
      if (!entry || entry.kind !== 'group') return win;
      return {
        ...win,
        layout: restoreNode(win.layout, entry.node, entry.hint),
        dock: win.dock.filter((d) => d !== entry),
      };
    }
    case 'group/rename': {
      const name = action.name.trim().slice(0, 80);
      return name ? patchGroup(win, action.groupId, { name }) : win;
    }
    case 'split/resize':
      return { ...win, layout: setSplitSizes(win.layout, action.splitId, action.sizes) };
    case 'window/sidebar':
      return { ...win, sidebarOpen: action.open };
  }
}

/** Changes a group's name or color, whether it is visible or collapsed. */
export function patchGroup(win: WindowState, groupId: string, patch: { name?: string; color?: string }): WindowState {
  return {
    ...win,
    layout: updateGroup(win.layout, groupId, patch),
    dock: win.dock.map((d) =>
      d.kind === 'group' && d.node.id === groupId ? { ...d, node: { ...d.node, ...patch } } : d,
    ),
  };
}

/** Adds a new session's tile; with a target it lands next to that node, otherwise at the end. */
export function addTile(win: WindowState, sessionId: string, target?: DropTarget): WindowState {
  return { ...win, layout: insertTile(win.layout, sessionId, target) };
}

/** Removes a session from the window entirely (layout, collapsed tiles and collapsed groups). */
export function dropSession(win: WindowState, sessionId: string): WindowState {
  const dock: DockEntry[] = [];
  for (const entry of win.dock) {
    if (entry.kind === 'tile') {
      if (entry.sessionId !== sessionId) dock.push(entry);
      continue;
    }
    const rest = removeTile(entry.node, sessionId).root;
    if (rest?.kind === 'group') dock.push({ ...entry, node: rest });
  }
  return { ...win, layout: removeTile(win.layout, sessionId).root, dock };
}

/** Takes a tile or group (by node key) out of a window's layout, e.g. to move it to another window. */
export function takeNode(win: WindowState, key: string): { win: WindowState; node: LayoutNode | null } {
  const { root, removed } = removeNode(win.layout, key);
  return removed ? { win: { ...win, layout: root }, node: removed } : { win, node: null };
}

/** Puts a tile or group into a window next to a target, or at the end without one. */
export function putNode(win: WindowState, node: LayoutNode, target: DropTarget | null): WindowState {
  if (!target || !findNode(win.layout, target.key)) return { ...win, layout: appendNode(win.layout, node) };
  const resolved = resolveTarget(win.layout, node, target);
  return { ...win, layout: insertNode(win.layout, node, resolved.key, resolved.side) };
}

/** Every group in the window, visible or collapsed. */
export function windowGroups(win: WindowState): GroupNode[] {
  return [...groups(win.layout), ...win.dock.flatMap((d) => (d.kind === 'group' ? [d.node] : []))];
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
    case 'node/move':
      return typeof a.key === 'string' && isDropTarget(a.target);
    case 'tile/collapse':
    case 'tile/expand':
      return typeof a.sessionId === 'string';
    case 'group/collapse':
    case 'group/expand':
      return typeof a.groupId === 'string';
    case 'group/rename':
      return typeof a.groupId === 'string' && typeof a.name === 'string';
    case 'split/resize':
      return typeof a.splitId === 'string' && Array.isArray(a.sizes) && a.sizes.every((s) => typeof s === 'number');
    case 'window/sidebar':
      return typeof a.open === 'boolean';
    default:
      return false;
  }
}
