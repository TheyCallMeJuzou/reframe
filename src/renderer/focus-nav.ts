// Moving the keyboard focus between tiles with arrow keys, by where the tiles are on screen.

export interface Box {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export type Direction = 'left' | 'right' | 'up' | 'down';

/**
 * The tile next to `fromId` in `direction`: of the tiles on that side, the nearest one; among equally near
 * ones (a column of tiles beside a tall one), the one level with the current tile's center.
 */
export function neighbor(boxes: Box[], fromId: string, direction: Direction): string | null {
  const from = boxes.find((b) => b.id === fromId);
  if (!from) return null;
  const horizontal = direction === 'left' || direction === 'right';
  const center = horizontal ? from.y + from.height / 2 : from.x + from.width / 2;
  let best: { id: string; gap: number; offset: number } | null = null;
  for (const box of boxes) {
    if (box.id === fromId) continue;
    const gap =
      direction === 'left'
        ? from.x - (box.x + box.width)
        : direction === 'right'
          ? box.x - (from.x + from.width)
          : direction === 'up'
            ? from.y - (box.y + box.height)
            : box.y - (from.y + from.height);
    if (gap < -1) continue; // not on that side (a pixel of overlap is rounding)
    const offset = horizontal ? outside(center, box.y, box.y + box.height) : outside(center, box.x, box.x + box.width);
    const nearer = !best || gap < best.gap - 1 || (Math.abs(gap - best.gap) <= 1 && offset < best.offset);
    if (nearer) best = { id: box.id, gap, offset };
  }
  return best?.id ?? null;
}

/** How far `value` lies outside the range from `min` to `max` (0 inside). */
const outside = (value: number, min: number, max: number) =>
  value < min ? min - value : value > max ? value - max : 0;
