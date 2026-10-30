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
  /**
   * Moves a tile or a whole group (by node key) next to a target in the same window. A collapsed one (dragged out of
   * the session list) comes out of the side bar, to the end of the layout without a target.
   */
  | { type: 'node/move'; key: string; target: DropTarget | null }
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
    case 'node/move': {
      if (!findNode(win.layout, action.key)) {
        const { win: rest, node } = takeNode(win, action.key);
        return node ? putNode(rest, node, action.target) : win;
      }
      return action.target ? { ...win, layout: moveNode(win.layout, action.key, action.target) } : win;
    }
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

/**
 * Takes a tile or group (by node key) out of a window, e.g. to move it to another window: out of its layout, or out
 * of the side bar (a tile of a collapsed group comes out alone, and the group stays collapsed with the rest).
 */
export function takeNode(win: WindowState, key: string): { win: WindowState; node: LayoutNode | null } {
  const { root, removed } = removeNode(win.layout, key);
  if (removed) return { win: { ...win, layout: root }, node: removed };
  for (const entry of win.dock) {
    const others = win.dock.filter((d) => d !== entry);
    if (entry.kind === 'tile') {
      if (tileKey(entry.sessionId) === key) return { win: { ...win, dock: others }, node: tile(entry.sessionId) };
      continue;
    }
    if (entry.node.id === key) return { win: { ...win, dock: others }, node: entry.node };
    const inGroup = removeNode(entry.node, key);
    if (!inGroup.removed) continue;
    const rest = inGroup.root?.kind === 'group' ? inGroup.root : null;
    const dock = rest ? win.dock.map((d) => (d === entry ? { ...entry, node: rest } : d)) : others;
    return { win: { ...win, dock }, node: inGroup.removed };
  }
  return { win, node: null };
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

/** The windows a start brings back: the ones with something in them, or the first one when all are empty. */
export function windowsToRestore(windows: WindowState[]): WindowState[] {
  const kept = windows.filter((w) => !isWindowEmpty(w));
  return kept.length > 0 ? kept : windows.slice(0, 1);
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
      return typeof a.key === 'string' && (a.target === null || isDropTarget(a.target));
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
