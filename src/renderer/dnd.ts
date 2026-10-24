import type { DragEvent as ReactDragEvent } from 'react';
import type { Side } from '@shared/layout';
import { GROUP_MIME, LIST_MIME, TILE_MIME, type ListItemRef } from '@shared/ipc';

/** A tile or group from a window, or a session from the session list. */
export type DragKind = 'tile' | 'group' | 'session';

/** What is being dragged: a tile or group (by node key) from a window. */
export interface NodeDragData {
  key: string;
  windowId: string;
}

const mimeOf = (kind: 'tile' | 'group') => (kind === 'group' ? GROUP_MIME : TILE_MIME);

export function setNodeDrag(event: ReactDragEvent, kind: 'tile' | 'group', data: NodeDragData): void {
  event.dataTransfer.setData(mimeOf(kind), JSON.stringify(data));
  event.dataTransfer.effectAllowed = 'move';
}

/** Tile or group being dragged, known from the MIME type even before the drop (and across windows). */
export function dragKind(event: ReactDragEvent | DragEvent): DragKind | null {
  const types = Array.from(event.dataTransfer?.types ?? []);
  if (types.includes(GROUP_MIME)) return 'group';
  if (types.includes(TILE_MIME)) return 'tile';
  if (types.includes(LIST_MIME)) return 'session';
  return null;
}

export function setListDrag(event: ReactDragEvent, item: ListItemRef): void {
  event.dataTransfer.setData(LIST_MIME, JSON.stringify(item));
  event.dataTransfer.effectAllowed = 'copyMove';
}

export function readNodeDrag(event: ReactDragEvent): NodeDragData | null {
  const kind = dragKind(event);
  if (kind !== 'tile' && kind !== 'group') return null;
  try {
    const data = JSON.parse(event.dataTransfer.getData(mimeOf(kind))) as NodeDragData;
    return typeof data.key === 'string' && typeof data.windowId === 'string' ? data : null;
  } catch {
    return null;
  }
}

/**
 * Handles a drop at `target` (null: no particular place, e.g. an empty window): list entries open there,
 * tiles and groups move. Returns whether the drop was one of ours.
 */
export function handleDrop(
  event: ReactDragEvent,
  windowId: string,
  target: { key: string; side: Side } | null,
): boolean {
  if (dragKind(event) === 'session') {
    try {
      const item = JSON.parse(event.dataTransfer.getData(LIST_MIME)) as ListItemRef;
      window.reframe.openSession({ item, target });
      return true;
    } catch {
      return false;
    }
  }
  const data = readNodeDrag(event);
  if (!data || data.key === target?.key) return false;
  dropNode(data, windowId, target);
  return true;
}

/** Drops into this window move within it; drops from another window transfer the node over. */
export function dropNode(data: NodeDragData, windowId: string, target: { key: string; side: Side } | null): void {
  if (data.windowId === windowId) {
    if (target) window.reframe.dispatch({ type: 'node/move', key: data.key, target });
  } else {
    window.reframe.transferNode({ key: data.key, fromWindowId: data.windowId, target });
  }
}

/** The edge of `rect` nearest to the pointer decides where a dropped tile goes. */
export function sideAt(rect: DOMRect, x: number, y: number): Side {
  const fx = (x - rect.left) / rect.width;
  const fy = (y - rect.top) / rect.height;
  const distances: [Side, number][] = [
    ['left', fx],
    ['right', 1 - fx],
    ['top', fy],
    ['bottom', 1 - fy],
  ];
  return distances.reduce((best, d) => (d[1] < best[1] ? d : best))[0];
}
