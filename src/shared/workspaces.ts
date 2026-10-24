// Workspaces: a window saved under a name, to be opened again later exactly like that, as a window of its own.
import { windowSessions } from './actions';
import { tileKey, type GroupNode, type LayoutNode, type RestoreHint } from './layout';
import type { SessionRecord, WindowState, Workspace } from './state';

/** A snapshot of a window: its tiles, groups, collapsed tiles and size, and the sessions in it. */
export function snapshotWindow(
  win: WindowState,
  sessions: Record<string, SessionRecord>,
  meta: { id: string; name: string; savedAt: number },
): Workspace {
  return {
    ...meta,
    window: {
      bounds: win.bounds,
      maximized: win.maximized,
      sidebarOpen: win.sidebarOpen,
      layout: win.layout,
      dock: win.dock,
    },
    sessions: windowSessions(win).flatMap((id) => (sessions[id] ? [sessions[id]] : [])),
  };
}

/**
 * The window that opens a workspace. Its tiles point to the sessions as reframe has them now (`sessionIds`: saved
 * id → current id), and a group whose id is in use in another window gets a new one, so groups stay unique.
 */
export function windowFromWorkspace(
  workspace: Workspace,
  windowId: string,
  sessionIds: ReadonlyMap<string, string>,
  groupIdsInUse: ReadonlySet<string>,
  newId: () => string,
): WindowState {
  // Restore hints name a collapsed tile's neighbor by its key, so renamed keys are followed there too.
  const keys = new Map<string, string>();
  for (const [from, to] of sessionIds) keys.set(tileKey(from), tileKey(to));
  const session = (id: string) => sessionIds.get(id) ?? id;
  const remap = (node: LayoutNode): LayoutNode => {
    if (node.kind === 'tile') return { kind: 'tile', sessionId: session(node.sessionId) };
    if (node.kind === 'group') {
      const id = groupIdsInUse.has(node.id) ? newId() : node.id;
      keys.set(node.id, id);
      return { ...node, id, child: remap(node.child) };
    }
    return { ...node, children: node.children.map(remap) };
  };
  const layout = workspace.window.layout && remap(workspace.window.layout);
  const dock = workspace.window.dock.map((entry) =>
    entry.kind === 'tile'
      ? { ...entry, sessionId: session(entry.sessionId) }
      : { ...entry, node: remap(entry.node) as GroupNode },
  );
  const hint = (h: RestoreHint | null) => h && { ...h, anchor: keys.get(h.anchor) ?? h.anchor };
  return {
    id: windowId,
    bounds: workspace.window.bounds,
    maximized: workspace.window.maximized,
    hidden: false,
    sidebarOpen: workspace.window.sidebarOpen,
    layout,
    dock: dock.map((entry) => ({ ...entry, hint: hint(entry.hint) })),
    workspaceId: workspace.id,
  };
}
