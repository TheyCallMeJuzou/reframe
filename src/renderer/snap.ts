// Snapping separators to a grid over the whole layout, so that separators of different splits (side by side, or
// in different groups) line up. Pure geometry: positions along one axis, in pixels.

/** Grid lines across the layout. */
export const GRID_STEPS = 12;

/** A split along its axis: where it starts, how long it is, and how thick each separator between its panels is. */
export interface SplitAxis {
  start: number;
  length: number;
  separator: number;
}

/** The grid along the same axis: the layout's start and length, divided into `steps`. */
export interface GridAxis {
  start: number;
  length: number;
  steps: number;
}

/**
 * The split's panel sizes (percentages) with every separator that moved since `before` on the nearest grid line,
 * unless that would push it past a neighbor. Separators that did not move stay where they are.
 */
export function snapSizes(sizes: number[], before: number[], split: SplitAxis, grid: GridAxis): number[] {
  if (sizes.length !== before.length || sizes.length < 2) return sizes;
  const available = split.length - (sizes.length - 1) * split.separator;
  if (available <= 0 || grid.length <= 0) return sizes;
  const step = grid.length / grid.steps;
  const prefix = (list: number[]) => list.map((_, i) => list.slice(0, i + 1).reduce((a, b) => a + b, 0));
  const now = prefix(sizes);
  const was = prefix(before);
  // Where separator i's middle is for a given share of the split before it, and back.
  const position = (share: number, i: number) =>
    split.start + (share / 100) * available + i * split.separator + split.separator / 2;
  const share = (x: number, i: number) =>
    ((x - split.start - i * split.separator - split.separator / 2) / available) * 100;

  const snapped = [...now];
  for (let i = 0; i < sizes.length - 1; i++) {
    if (Math.abs(now[i]! - was[i]!) < 0.01) continue;
    const line = grid.start + Math.round((position(now[i]!, i) - grid.start) / step) * step;
    const target = share(line, i);
    const low = i > 0 ? snapped[i - 1]! : 0;
    const high = now[i + 1]!;
    if (target > low && target < high) snapped[i] = target;
  }
  return snapped.map((end, i) => end - (i > 0 ? snapped[i - 1]! : 0));
}
