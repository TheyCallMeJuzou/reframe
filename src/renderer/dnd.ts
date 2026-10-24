import type { DragEvent as ReactDragEvent } from 'react';
import type { Side } from '@shared/layout';
import { GROUP_MIME, TILE_MIME } from '@shared/ipc';

export type DragKind = 'tile' | 'group';

/** What is being dragged: a tile or group (by node key) from a window. */
export interface NodeDragData {
  key: string;
  windowId: string;
}

const mimeOf = (kind: DragKind) => (kind === 'group' ? GROUP_MIME : TILE_MIME);

export function setNodeDrag(event: ReactDragEvent, kind: DragKind, data: NodeDragData): void {
  event.dataTransfer.setData(mimeOf(kind), JSON.stringify(data));
  event.dataTransfer.effectAllowed = 'move';
}

/** Tile or group being dragged, known from the MIME type even before the drop (and across windows). */
export function dragKind(event: ReactDragEvent | DragEvent): DragKind | null {
  const types = Array.from(event.dataTransfer?.types ?? []);
  if (types.includes(GROUP_MIME)) return 'group';
  if (types.includes(TILE_MIME)) return 'tile';
  return null;
}

export function readNodeDrag(event: ReactDragEvent): NodeDragData | null {
  const kind = dragKind(event);
  if (!kind) return null;
  try {
    const data = JSON.parse(event.dataTransfer.getData(mimeOf(kind))) as NodeDragData;
    return typeof data.key === 'string' && typeof data.windowId === 'string' ? data : null;
  } catch {
    return null;
  }
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
