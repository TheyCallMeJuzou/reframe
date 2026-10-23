import type { DragEvent as ReactDragEvent } from 'react';
import type { Side } from '@shared/layout';
import { TILE_MIME } from '@shared/ipc';

export interface TileDragData {
  sessionId: string;
  windowId: string;
}

export function setTileDrag(event: ReactDragEvent, data: TileDragData): void {
  event.dataTransfer.setData(TILE_MIME, JSON.stringify(data));
  event.dataTransfer.effectAllowed = 'move';
}

export function isTileDrag(event: ReactDragEvent | DragEvent): boolean {
  return !!event.dataTransfer && Array.from(event.dataTransfer.types).includes(TILE_MIME);
}

export function readTileDrag(event: ReactDragEvent): TileDragData | null {
  try {
    const data = JSON.parse(event.dataTransfer.getData(TILE_MIME)) as TileDragData;
    return typeof data.sessionId === 'string' ? data : null;
  } catch {
    return null;
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
