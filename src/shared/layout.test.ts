import { describe, expect, it } from 'vitest';
import {
  appendNode,
  insertTile,
  leaves,
  moveTile,
  normalize,
  removeTile,
  restoreNode,
  setSplitSizes,
  tile,
  tileKey,
  type LayoutNode,
  type SplitNode,
} from './layout';

const split = (dir: 'row' | 'col', children: LayoutNode[], sizes?: number[]): SplitNode => ({
  kind: 'split',
  id: `s-${Math.random()}`,
  dir,
  children,
  sizes: sizes ?? children.map(() => 100 / children.length),
});

/** Structure without ids or exact sizes, e.g. row(a, col(b, c)). */
function shape(node: LayoutNode | null): string {
  if (!node) return '∅';
  if (node.kind === 'tile') return node.sessionId;
  if (node.kind === 'group') return `group(${shape(node.child)})`;
  return `${node.dir}(${node.children.map(shape).join(', ')})`;
}

function sizesOf(node: LayoutNode | null): number[] {
  return node && node.kind === 'split' ? node.sizes.map((s) => Math.round(s * 10) / 10) : [];
}

describe('insertTile', () => {
  it('makes the first tile the root', () => {
    expect(shape(insertTile(null, 'a'))).toBe('a');
  });

  it('appends to the end of the top row without a target', () => {
    let root = insertTile(null, 'a');
    root = insertTile(root, 'b');
    root = insertTile(root, 'c');
    expect(shape(root)).toBe('row(a, b, c)');
    expect(sizesOf(root)).toEqual([33.3, 33.3, 33.3]);
  });

  it('places a tile on each side of a target', () => {
    const base = tile('a');
    expect(shape(insertTile(base, 'b', { key: tileKey('a'), side: 'left' }))).toBe('row(b, a)');
    expect(shape(insertTile(base, 'b', { key: tileKey('a'), side: 'right' }))).toBe('row(a, b)');
    expect(shape(insertTile(base, 'b', { key: tileKey('a'), side: 'top' }))).toBe('col(b, a)');
    expect(shape(insertTile(base, 'b', { key: tileKey('a'), side: 'bottom' }))).toBe('col(a, b)');
  });

  it('splits only the target slot and flattens same-direction splits', () => {
    const root = split('row', [tile('a'), tile('b')], [50, 50]);
    const next = insertTile(root, 'c', { key: tileKey('b'), side: 'right' });
    expect(shape(next)).toBe('row(a, b, c)');
    expect(sizesOf(next)).toEqual([50, 25, 25]);
  });

  it('nests a perpendicular split inside the target slot', () => {
    const root = split('row', [tile('a'), tile('b')]);
    expect(shape(insertTile(root, 'c', { key: tileKey('b'), side: 'bottom' }))).toBe('row(a, col(b, c))');
  });

  it('appends when the target is missing', () => {
    expect(shape(insertTile(tile('a'), 'b', { key: tileKey('zzz'), side: 'top' }))).toBe('row(a, b)');
  });
});

describe('removeTile', () => {
  it('collapses a split that is left with one child', () => {
    const { root, removed } = removeTile(split('row', [tile('a'), tile('b')]), 'a');
    expect(shape(root)).toBe('b');
    expect(removed).toEqual(tile('a'));
  });

  it('redistributes the freed space', () => {
    const { root } = removeTile(split('row', [tile('a'), tile('b'), tile('c')], [50, 25, 25]), 'c');
    expect(shape(root)).toBe('row(a, b)');
    expect(sizesOf(root)).toEqual([66.7, 33.3]);
  });

  it('empties the layout when the last tile goes', () => {
    expect(removeTile(tile('a'), 'a').root).toBeNull();
  });

  it('records the neighbor as restore anchor', () => {
    const { hint } = removeTile(split('row', [tile('a'), tile('b'), tile('c')], [50, 30, 20]), 'b');
    expect(hint).toEqual({ anchor: tileKey('a'), side: 'right', ratio: 30 / 80, share: 0.3 });
    const first = removeTile(split('col', [tile('a'), tile('b')]), 'a').hint;
    expect(first).toEqual({ anchor: tileKey('b'), side: 'top', ratio: 0.5, share: 0.5 });
  });

  it('removes a group that becomes empty', () => {
    const root = split('row', [tile('a'), { kind: 'group', id: 'g', name: 'G', color: '#f00', child: tile('b') }]);
    expect(shape(removeTile(root, 'b').root)).toBe('a');
  });
});

describe('restoreNode', () => {
  it('puts a collapsed tile back where it was', () => {
    const before = split('row', [tile('a'), split('col', [tile('b'), tile('c')])]);
    const { root, removed, hint } = removeTile(before, 'c');
    expect(shape(root)).toBe('row(a, b)');
    const after = restoreNode(root, removed!, hint);
    expect(shape(after)).toBe('row(a, col(b, c))');
  });

  it('keeps the remembered share', () => {
    const before = split('row', [tile('a'), tile('b'), tile('c')], [50, 30, 20]);
    const { root, removed, hint } = removeTile(before, 'b');
    expect(sizesOf(restoreNode(root, removed!, hint)).map(Math.round)).toEqual([50, 30, 20]);
  });

  it('appends when the anchor is gone', () => {
    const { root, removed, hint } = removeTile(split('row', [tile('a'), tile('b')]), 'b');
    const withoutAnchor = removeTile(appendNode(root, tile('x')), 'a').root;
    expect(shape(restoreNode(withoutAnchor, removed!, hint))).toBe('row(x, b)');
  });
});

describe('moveTile', () => {
  it('moves a tile next to another one', () => {
    const root = split('row', [tile('a'), tile('b'), tile('c')]);
    expect(shape(moveTile(root, 'a', { key: tileKey('c'), side: 'bottom' }))).toBe('row(b, col(c, a))');
  });

  it('ignores a drop onto itself', () => {
    const root = split('row', [tile('a'), tile('b')]);
    expect(moveTile(root, 'a', { key: tileKey('a'), side: 'left' })).toBe(root);
  });

  it('keeps every tile exactly once', () => {
    let root: LayoutNode | null = split('row', [tile('a'), split('col', [tile('b'), tile('c')]), tile('d')]);
    root = moveTile(root, 'b', { key: tileKey('d'), side: 'top' });
    root = moveTile(root, 'd', { key: tileKey('a'), side: 'left' });
    expect(leaves(root).sort()).toEqual(['a', 'b', 'c', 'd']);
  });
});

describe('normalize and sizes', () => {
  it('flattens nested same-direction splits with proportional sizes', () => {
    const nested = split('row', [tile('a'), split('row', [tile('b'), tile('c')], [50, 50])], [50, 50]);
    const flat = normalize(nested);
    expect(shape(flat)).toBe('row(a, b, c)');
    expect(sizesOf(flat)).toEqual([50, 25, 25]);
  });

  it('accepts only matching, finite sizes', () => {
    const root = split('row', [tile('a'), tile('b')]);
    expect(sizesOf(setSplitSizes(root, root.id, [1, 3]))).toEqual([25, 75]);
    expect(setSplitSizes(root, root.id, [1, 2, 3])).toEqual(root);
    expect(setSplitSizes(root, root.id, [Number.NaN, 1])).toEqual(root);
  });
});
