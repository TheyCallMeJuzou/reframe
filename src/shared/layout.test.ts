import { describe, expect, it } from 'vitest';
import {
  addToGroup,
  appendNode,
  dissolveGroup,
  groupOf,
  groups,
  groupTile,
  insertTile,
  leaves,
  moveNode,
  moveTile,
  normalize,
  removeFromGroup,
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

  it('without a target: two columns that fill in turn (3 below 2, 4 below 1, 5 below 3)', () => {
    let root = insertTile(null, 'a');
    root = insertTile(root, 'b');
    expect(shape(root)).toBe('row(a, b)');
    expect(sizesOf(root)).toEqual([50, 50]);
    root = insertTile(root, 'c');
    expect(shape(root)).toBe('row(a, col(b, c))');
    root = insertTile(root, 'd');
    expect(shape(root)).toBe('row(col(a, d), col(b, c))');
    root = insertTile(root, 'e');
    root = insertTile(root, 'f');
    expect(shape(root)).toBe('row(col(a, d, f), col(b, c, e))');
    expect(sizesOf(root)).toEqual([50, 50]);
    expect(sizesOf((root as SplitNode).children[1]!)).toEqual([33.3, 33.3, 33.3]);
  });

  it('adds to the emptiest column of a layout the user built, keeping its proportions', () => {
    expect(shape(appendNode(split('row', [tile('a'), tile('b'), tile('c')]), tile('x')))).toBe('row(a, b, col(c, x))');
    const threeColumns = split('row', [
      split('col', [tile('a'), tile('b')]),
      tile('c'),
      split('col', [tile('d'), tile('e')]),
    ]);
    expect(shape(appendNode(threeColumns, tile('x')))).toBe('row(col(a, b), col(c, x), col(d, e))');
    expect(shape(appendNode(split('col', [tile('a'), tile('b')]), tile('x')))).toBe('row(col(a, b), x)');
    const resized = appendNode(
      split('row', [split('col', [tile('a'), tile('d')]), split('col', [tile('b'), tile('c')], [70, 30])]),
      tile('x'),
    );
    expect(sizesOf((resized as SplitNode).children[1]!)).toEqual([46.7, 20, 33.3]);
  });

  it('puts a new tile below a group on the right, not into it', () => {
    const G = { id: 'g', name: 'G', color: '#f00' };
    const root = groupTile(split('row', [tile('a'), tile('b')]), 'b', G);
    expect(shape(appendNode(root, tile('x')))).toBe('row(a, col(group(b), x))');
    expect(shape(appendNode(groupTile(tile('a'), 'a', G), tile('x')))).toBe('row(group(a), x)');
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

describe('groups', () => {
  const G = { id: 'g', name: 'G', color: '#f00' };
  const H = { id: 'h', name: 'H', color: '#00f' };

  it('frames a tile and grows by adding tiles after its last one', () => {
    let root: LayoutNode | null = split('row', [tile('a'), tile('b'), tile('c')]);
    root = groupTile(root, 'a', G);
    expect(shape(root)).toBe('row(group(a), b, c)');
    root = addToGroup(root, 'c', 'g');
    expect(shape(root)).toBe('row(group(row(a, c)), b)');
    expect(groupOf(root, tileKey('c'))?.id).toBe('g');
    expect(groupOf(root, tileKey('b'))).toBeNull();
    expect(groupTile(root, 'a', H)).toBe(root); // already grouped
  });

  it('grows around a neighboring tile instead of moving it', () => {
    let root: LayoutNode | null = groupTile(split('col', [tile('a'), tile('b')], [30, 70]), 'b', G);
    root = addToGroup(root, 'a', 'g');
    expect(shape(root)).toBe('group(col(a, b))');
    expect(root?.kind === 'group' ? sizesOf(root.child) : []).toEqual([30, 70]);

    root = groupTile(split('row', [tile('c'), split('col', [tile('a'), tile('b'), tile('d')], [20, 30, 50])]), 'a', G);
    root = groupTile(root, 'b', H);
    root = addToGroup(root, 'b', 'g');
    expect(shape(root)).toBe('row(c, col(group(col(a, b)), d))');
    const column = root?.kind === 'split' ? root.children[1]! : null;
    expect(sizesOf(column)).toEqual([50, 50]);
    // Inside the frame, a and b keep their heights (20 : 30 of the column).
    const grown = column?.kind === 'split' ? column.children[0]! : null;
    expect(grown?.kind === 'group' ? sizesOf(grown.child) : []).toEqual([40, 60]);
    expect(groups(root).map((g) => g.id)).toEqual(['g']);
  });

  it('takes a tile out next to its group, and an empty group disappears', () => {
    let root = groupTile(split('row', [tile('a'), tile('b')]), 'a', G);
    root = addToGroup(root, 'b', 'g');
    root = removeFromGroup(root, 'b');
    expect(shape(root)).toBe('row(group(a), b)');
    root = removeFromGroup(root, 'a');
    expect(groups(root)).toEqual([]);
    expect(leaves(root).sort()).toEqual(['a', 'b']);
  });

  it('dissolves a group into its tiles', () => {
    let root = groupTile(split('row', [tile('a'), tile('b'), tile('c')]), 'a', G);
    root = addToGroup(root, 'b', 'g');
    expect(shape(dissolveGroup(root, 'g'))).toBe('row(a, b, c)');
  });

  it('moves a whole group, and a dragged tile joins or leaves groups by where it lands', () => {
    let root = groupTile(split('row', [tile('a'), tile('b'), tile('c')]), 'a', G);
    root = moveNode(root, 'g', { key: tileKey('c'), side: 'bottom' });
    expect(shape(root)).toBe('row(b, col(c, group(a)))');
    root = moveTile(root, 'b', { key: tileKey('a'), side: 'right' });
    expect(groupOf(root, tileKey('b'))?.id).toBe('g');
    root = moveTile(root, 'b', { key: tileKey('c'), side: 'left' });
    expect(groupOf(root, tileKey('b'))).toBeNull();
  });

  it('never nests groups and ignores drops of a group onto itself', () => {
    let root = groupTile(split('row', [tile('a'), tile('b')]), 'a', G);
    root = groupTile(root, 'b', H);
    const moved = moveNode(root, 'h', { key: tileKey('a'), side: 'bottom' });
    expect(shape(moved)).toBe('col(group(a), group(b))');
    expect(moveNode(root, 'g', { key: tileKey('a'), side: 'left' })).toBe(root);
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
