import { describe, expect, it } from 'vitest';
import { neighbor, type Box } from './focus-nav';

const box = (id: string, x: number, y: number, width: number, height: number): Box => ({ id, x, y, width, height });

describe('neighbor', () => {
  // a on the left, b / c / d stacked on the right (4 px splitters between them).
  const stacked = [
    box('a', 0, 0, 500, 600),
    box('b', 504, 0, 500, 196),
    box('c', 504, 200, 500, 196),
    box('d', 504, 404, 500, 196),
  ];

  it('moves within a column and across to the tall tile', () => {
    expect(neighbor(stacked, 'b', 'down')).toBe('c');
    expect(neighbor(stacked, 'd', 'up')).toBe('c');
    expect(neighbor(stacked, 'c', 'left')).toBe('a');
    expect(neighbor(stacked, 'd', 'left')).toBe('a');
  });

  it('goes from the tall tile to the one level with its center', () => {
    expect(neighbor(stacked, 'a', 'right')).toBe('c');
  });

  it('prefers the tile straight across in a grid over a diagonal one', () => {
    const grid = [
      box('a', 0, 0, 500, 300),
      box('b', 0, 304, 500, 300),
      box('c', 504, 0, 500, 300),
      box('d', 504, 304, 500, 300),
    ];
    expect(neighbor(grid, 'a', 'down')).toBe('b');
    expect(neighbor(grid, 'a', 'right')).toBe('c');
    expect(neighbor(grid, 'd', 'up')).toBe('c');
  });

  it('stays put at the edge, or for an unknown tile', () => {
    expect(neighbor(stacked, 'a', 'left')).toBeNull();
    expect(neighbor(stacked, 'b', 'up')).toBeNull();
    expect(neighbor(stacked, 'zzz', 'up')).toBeNull();
  });
});
