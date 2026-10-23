// Pure operations on a window's split layout. A layout is a tree of splits (row = side by side,
// col = stacked), groups (a named, colored frame around a subtree) and tiles (one session each).
// Every operation returns a new, normalized tree: no split has fewer than two children, nested
// splits in the same direction are flattened, and split sizes are percentages that sum to 100.

export type Side = 'left' | 'right' | 'top' | 'bottom';
export type SplitDir = 'row' | 'col';

export interface TileNode {
  kind: 'tile';
  sessionId: string;
}

export interface SplitNode {
  kind: 'split';
  id: string;
  dir: SplitDir;
  children: LayoutNode[];
  sizes: number[];
}

export interface GroupNode {
  kind: 'group';
  id: string;
  name: string;
  color: string;
  child: LayoutNode;
}

export type LayoutNode = TileNode | SplitNode | GroupNode;

/** Where a tile is dropped: next to another node, on one of its sides. */
export interface DropTarget {
  /** Key of the target node, see {@link nodeKey}. */
  key: string;
  side: Side;
}

/** Remembers where a removed tile sat, so it can be put back there. */
export interface RestoreHint {
  /** Key of the neighbor the tile was next to. */
  anchor: string;
  side: Side;
  /** The tile's share of the space it shared with the anchor (0..1). */
  ratio: number;
  /** The tile's share of its whole split (0..1), restored when it goes back into a split of that direction. */
  share?: number;
}

const newId = (): string => globalThis.crypto.randomUUID();

export const tileKey = (sessionId: string): string => `tile:${sessionId}`;

export function nodeKey(node: LayoutNode): string {
  return node.kind === 'tile' ? tileKey(node.sessionId) : node.id;
}

export function tile(sessionId: string): TileNode {
  return { kind: 'tile', sessionId };
}

export function leaves(node: LayoutNode | null): string[] {
  if (!node) return [];
  if (node.kind === 'tile') return [node.sessionId];
  if (node.kind === 'group') return leaves(node.child);
  return node.children.flatMap(leaves);
}

export function findNode(node: LayoutNode | null, key: string): LayoutNode | null {
  if (!node) return null;
  if (nodeKey(node) === key) return node;
  if (node.kind === 'group') return findNode(node.child, key);
  if (node.kind === 'split') {
    for (const child of node.children) {
      const found = findNode(child, key);
      if (found) return found;
    }
  }
  return null;
}

const sideDir = (side: Side): SplitDir => (side === 'left' || side === 'right' ? 'row' : 'col');
const sideFirst = (side: Side): boolean => side === 'left' || side === 'top';

function scaleTo100(sizes: number[]): number[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  if (sizes.length === 0) return [];
  if (!(total > 0)) return sizes.map(() => 100 / sizes.length);
  return sizes.map((s) => (s / total) * 100);
}

export function normalize(node: LayoutNode): LayoutNode {
  if (node.kind === 'tile') return node;
  if (node.kind === 'group') return { ...node, child: normalize(node.child) };

  const sizes = node.sizes.length === node.children.length ? node.sizes : node.children.map(() => 1);
  const children: LayoutNode[] = [];
  const childSizes: number[] = [];
  node.children.forEach((raw, i) => {
    const child = normalize(raw);
    const size = sizes[i] ?? 0;
    if (child.kind === 'split' && child.dir === node.dir) {
      const inner = scaleTo100(child.sizes);
      child.children.forEach((grandchild, j) => {
        children.push(grandchild);
        childSizes.push((size * (inner[j] ?? 0)) / 100);
      });
    } else {
      children.push(child);
      childSizes.push(size);
    }
  });
  if (children.length === 1) return children[0]!;
  return { ...node, children, sizes: scaleTo100(childSizes) };
}

function replaceNode(node: LayoutNode, key: string, replace: (n: LayoutNode) => LayoutNode): LayoutNode {
  if (nodeKey(node) === key) return replace(node);
  if (node.kind === 'group') return { ...node, child: replaceNode(node.child, key, replace) };
  if (node.kind === 'split') return { ...node, children: node.children.map((c) => replaceNode(c, key, replace)) };
  return node;
}

/** Puts `node` next to the node with key `anchor` on `side`; `ratio` is the new node's share (0..1). */
export function insertNode(
  root: LayoutNode | null,
  node: LayoutNode,
  anchor?: string,
  side: Side = 'right',
  ratio = 0.5,
): LayoutNode {
  if (!root) return node;
  const r = Math.min(0.9, Math.max(0.1, ratio));
  const wrap = (existing: LayoutNode): LayoutNode => {
    const first = sideFirst(side);
    return {
      kind: 'split',
      id: newId(),
      dir: sideDir(side),
      children: first ? [node, existing] : [existing, node],
      sizes: first ? [r * 100, (1 - r) * 100] : [(1 - r) * 100, r * 100],
    };
  };
  if (anchor && findNode(root, anchor)) return normalize(replaceNode(root, anchor, wrap));
  return appendNode(root, node);
}

/** Adds a node at the end of the top level, sharing space evenly with the existing columns. */
export function appendNode(root: LayoutNode | null, node: LayoutNode): LayoutNode {
  if (!root) return node;
  if (root.kind === 'split' && root.dir === 'row') {
    const n = root.children.length;
    const sizes = [...scaleTo100(root.sizes).map((s) => (s * n) / (n + 1)), 100 / (n + 1)];
    return normalize({ ...root, children: [...root.children, node], sizes });
  }
  return normalize({ kind: 'split', id: newId(), dir: 'row', children: [root, node], sizes: [50, 50] });
}

export function insertTile(root: LayoutNode | null, sessionId: string, target?: DropTarget): LayoutNode {
  return insertNode(root, tile(sessionId), target?.key, target?.side);
}

/** Removes the node with `key` and returns the remaining tree plus a hint for putting it back. */
export function removeNode(
  root: LayoutNode | null,
  key: string,
): { root: LayoutNode | null; removed: LayoutNode | null; hint: RestoreHint | null } {
  if (!root) return { root: null, removed: null, hint: null };
  if (nodeKey(root) === key) return { root: null, removed: root, hint: null };

  let removed: LayoutNode | null = null;
  let hint: RestoreHint | null = null;

  const walk = (node: LayoutNode): LayoutNode | null => {
    if (node.kind === 'tile') return node;
    if (node.kind === 'group') {
      if (nodeKey(node.child) === key) {
        removed = node.child;
        return null; // an empty group disappears
      }
      const child = walk(node.child);
      return child ? { ...node, child } : null;
    }
    const index = node.children.findIndex((c) => nodeKey(c) === key);
    if (index >= 0) {
      removed = node.children[index]!;
      const neighborIndex = index > 0 ? index - 1 : index + 1;
      const neighbor = node.children[neighborIndex]!;
      const own = node.sizes[index] ?? 0;
      const other = node.sizes[neighborIndex] ?? 0;
      const before = neighborIndex < index;
      const total = node.sizes.reduce((a, b) => a + b, 0);
      hint = {
        anchor: nodeKey(neighbor),
        side: node.dir === 'row' ? (before ? 'right' : 'left') : before ? 'bottom' : 'top',
        ratio: own + other > 0 ? own / (own + other) : 0.5,
        share: total > 0 ? own / total : undefined,
      };
      return {
        ...node,
        children: node.children.filter((_, i) => i !== index),
        sizes: node.sizes.filter((_, i) => i !== index),
      };
    }
    const children: LayoutNode[] = [];
    const sizes: number[] = [];
    node.children.forEach((c, i) => {
      const next = walk(c);
      if (next) {
        children.push(next);
        sizes.push(node.sizes[i] ?? 0);
      }
    });
    return children.length ? { ...node, children, sizes } : null;
  };

  const next = walk(root);
  return { root: next ? normalize(next) : null, removed, hint };
}

export function removeTile(root: LayoutNode | null, sessionId: string) {
  return removeNode(root, tileKey(sessionId));
}

/** Puts a node back where a {@link RestoreHint} says; falls back to appending if the anchor is gone. */
export function restoreNode(root: LayoutNode | null, node: LayoutNode, hint: RestoreHint | null): LayoutNode {
  if (!hint || !root || !findNode(root, hint.anchor)) return appendNode(root, node);
  const restored = insertNode(root, node, hint.anchor, hint.side, hint.ratio);
  const share = hint.share;
  const parent = findParentSplit(restored, nodeKey(node));
  if (!parent || share === undefined || parent.dir !== sideDir(hint.side) || !(share > 0 && share < 1)) return restored;
  // Take the remembered share back from all siblings, as they all grew when the tile left. Insertion
  // carved the tile out of the anchor's slot, so the anchor gets that space back before scaling.
  const index = parent.children.findIndex((c) => nodeKey(c) === nodeKey(node));
  const anchorIndex = parent.children.findIndex((c) => nodeKey(c) === hint.anchor);
  const before = parent.sizes.map((s, i) => (i === anchorIndex ? s + (parent.sizes[index] ?? 0) : s));
  const others = scaleTo100(before.filter((_, i) => i !== index));
  const sizes: number[] = [];
  let o = 0;
  for (let i = 0; i < parent.children.length; i++) {
    sizes.push(i === index ? share * 100 : (others[o++] ?? 0) * (1 - share));
  }
  return setSplitSizes(restored, parent.id, sizes) ?? restored;
}

function findParentSplit(node: LayoutNode, key: string): SplitNode | null {
  if (node.kind === 'tile') return null;
  if (node.kind === 'group') return findParentSplit(node.child, key);
  if (node.children.some((c) => nodeKey(c) === key)) return node;
  for (const child of node.children) {
    const found = findParentSplit(child, key);
    if (found) return found;
  }
  return null;
}

export function moveTile(root: LayoutNode | null, sessionId: string, target: DropTarget): LayoutNode | null {
  if (target.key === tileKey(sessionId)) return root;
  const { root: without, removed } = removeTile(root, sessionId);
  if (!removed) return root;
  return insertNode(without, removed, target.key, target.side);
}

export function setSplitSizes(root: LayoutNode | null, splitId: string, sizes: number[]): LayoutNode | null {
  if (!root) return root;
  return replaceNode(root, splitId, (node) =>
    node.kind === 'split' && sizes.length === node.children.length && sizes.every((s) => Number.isFinite(s) && s >= 0)
      ? { ...node, sizes: scaleTo100(sizes) }
      : node,
  );
}
