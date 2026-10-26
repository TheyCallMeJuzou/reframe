import type { DragEvent as ReactDragEvent } from 'react';
import { tileKey, type Side } from '@shared/layout';
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

export function setListDrag(event: ReactDragEvent, items: ListItemRef[]): void {
  event.dataTransfer.setData(LIST_MIME, JSON.stringify(items));
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
      const items = JSON.parse(event.dataTransfer.getData(LIST_MIME)) as ListItemRef[];
      if (items.length === 1) window.reframe.openSession({ item: items[0]!, target });
      else window.reframe.openSessions({ items, mode: 'open', target });
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

/**
 * Drops into this window move within it (a collapsed session from the list, also onto the empty area); drops from
 * another window transfer the node over.
 */
export function dropNode(data: NodeDragData, windowId: string, target: { key: string; side: Side } | null): void {
  if (data.windowId === windowId) {
    window.reframe.dispatch({ type: 'node/move', key: data.key, target });
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

export interface PreviewRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The drop target under a point that no DOM drag event describes (a foreign window dragged over us). */
export function dropTargetAt(
  x: number,
  y: number,
): { target: { key: string; side: Side } | null; rect: PreviewRect | null } {
  const element = document.elementFromPoint(x, y);
  const tile = element?.closest<HTMLElement>('.tile[data-session]');
  if (tile?.dataset.session) {
    const rect = (tile.querySelector('.tile-body') ?? tile).getBoundingClientRect();
    const side = sideAt(rect, x, y);
    return { target: { key: tileKey(tile.dataset.session), side }, rect: half(rect, side) };
  }
  // On a group's bar or frame (not on one of its tiles): into the group, after its last tile.
  const group = element?.closest<HTMLElement>('.group[data-group]');
  const last = group ? [...group.querySelectorAll<HTMLElement>('.tile[data-session]')].at(-1) : undefined;
  if (group && last?.dataset.session) {
    const body = group.querySelector('.group-body') ?? group;
    return { target: { key: tileKey(last.dataset.session), side: 'right' }, rect: body.getBoundingClientRect() };
  }
  const empty = element?.closest('.empty-area');
  return { target: null, rect: empty ? empty.getBoundingClientRect() : null };
}

function half(r: DOMRect, side: Side): PreviewRect {
  const w = r.width / 2;
  const h = r.height / 2;
  if (side === 'left') return { left: r.left, top: r.top, width: w, height: r.height };
  if (side === 'right') return { left: r.left + w, top: r.top, width: w, height: r.height };
  if (side === 'top') return { left: r.left, top: r.top, width: r.width, height: h };
  return { left: r.left, top: r.top + h, width: r.width, height: h };
}
