import { describe, expect, it } from 'vitest';
import { snapSizes } from './snap';

const grid = { start: 0, length: 1200, steps: 12 };
const round = (sizes: number[]) => sizes.map((s) => Math.round(s * 100) / 100);

describe('snapSizes', () => {
  it('puts the separator being dragged on the nearest grid line', () => {
    // A split over the whole layout without separator thickness: a grid line every 100 px (8.33 %).
    const whole = { start: 0, length: 1200, separator: 0 };
    expect(round(snapSizes([37, 63], [50, 50], whole, grid))).toEqual([33.33, 66.67]);
    expect(round(snapSizes([45.9, 54.1], [50, 50], whole, grid))).toEqual([50, 50]);
  });

  it('lines up with the grid of the whole layout, also for a split that covers only part of it', () => {
    // The right half of the layout (600..1200): grid lines at 700, 800, … are at 1/6, 2/6, … of the split.
    const rightHalf = { start: 600, length: 600, separator: 0 };
    expect(round(snapSizes([30, 70], [50, 50], rightHalf, grid))).toEqual([33.33, 66.67]);
  });

  it('counts the separators: their middle lands on the line', () => {
    const withSeparator = { start: 0, length: 1206, separator: 6 };
    const [first] = snapSizes([26, 74], [50, 50], withSeparator, grid);
    // The first panel ends 3 px before the line at 300, so the separator's middle is on it.
    expect(Math.round((first! / 100) * 1200)).toBe(297);
  });

  it('leaves separators that did not move, and one that would pass its neighbor', () => {
    const whole = { start: 0, length: 1200, separator: 0 };
    // Three panels; only the second separator moved (from 66.67 to 80.5 %).
    expect(round(snapSizes([33.33, 47.17, 19.5], [33.33, 33.33, 33.34], whole, grid))).toEqual([33.33, 50, 16.67]);
    // The nearest line (8.33 %) is behind the first separator (at 10 %): it stays.
    expect(round(snapSizes([10, 0.5, 89.5], [10, 20, 70], whole, grid))).toEqual([10, 0.5, 89.5]);
  });
});
