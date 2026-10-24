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

/**
 * Adds a node where new tiles go when no place is given: the first fills the window, the second comes to its
 * right (half and half), and every further one at the bottom of the column with the fewest tiles (the right one
 * when they are even), so two columns fill in turn: 3 below 2, 4 below 1, 5 below 3. That rule also holds for a
 * layout the user built (with more columns, or a single one, which gets a second); a group in the chosen
 * column gets the node below it, not into it.
 */
export function appendNode(root: LayoutNode | null, node: LayoutNode): LayoutNode {
  if (!root) return node;
  if (root.kind === 'split' && root.dir === 'row') {
    const count = (column: LayoutNode) =>
      column.kind === 'split' && column.dir === 'col' ? column.children.length : 1;
    let target = root.children.length - 1;
    root.children.forEach((column, i) => {
      if (count(column) < count(root.children[target]!)) target = i;
    });
    const children = root.children.map((column, i) => (i === target ? stackBelow(column, node) : column));
    return normalize({ ...root, children });
  }
  return normalize({ kind: 'split', id: newId(), dir: 'row', children: [root, node], sizes: [50, 50] });
}

/** `node` at the bottom of `column` (made one if needed); the heights there keep their proportions. */
function stackBelow(column: LayoutNode, node: LayoutNode): LayoutNode {
  if (column.kind === 'split' && column.dir === 'col') {
    const n = column.children.length;
    const sizes = [...scaleTo100(column.sizes).map((s) => (s * n) / (n + 1)), 100 / (n + 1)];
    return normalize({ ...column, children: [...column.children, node], sizes });
  }
  return { kind: 'split', id: newId(), dir: 'col', children: [column, node], sizes: [50, 50] };
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

/** Moves the node with `key` (a tile or a whole group) next to the target. */
export function moveNode(root: LayoutNode | null, key: string, target: DropTarget): LayoutNode | null {
  const node = findNode(root, key);
  // Dropping a node onto itself or a group onto one of its own tiles changes nothing.
  if (!node || findNode(node, target.key)) return root;
  const { root: without, removed } = removeNode(root, key);
  if (!removed) return root;
  const resolved = resolveTarget(without, removed, target);
  return insertNode(without, removed, resolved.key, resolved.side);
}

export function moveTile(root: LayoutNode | null, sessionId: string, target: DropTarget): LayoutNode | null {
  return moveNode(root, tileKey(sessionId), target);
}

/** Groups do not nest: a group dropped next to a grouped tile lands next to that tile's group instead. */
export function resolveTarget(root: LayoutNode | null, node: LayoutNode, target: DropTarget): DropTarget {
  if (node.kind !== 'group') return target;
  const group = groupOf(root, target.key);
  return group ? { key: group.id, side: target.side } : target;
}

// ---------- groups ----------

/** The group the node with `key` sits in, if any. */
export function groupOf(root: LayoutNode | null, key: string): GroupNode | null {
  const walk = (node: LayoutNode, group: GroupNode | null): GroupNode | null | undefined => {
    if (nodeKey(node) === key) return group;
    if (node.kind === 'group') return walk(node.child, node);
    if (node.kind === 'split') {
      for (const child of node.children) {
        const found = walk(child, group);
        if (found !== undefined) return found;
      }
    }
    return undefined;
  };
  return root ? (walk(root, null) ?? null) : null;
}

export function groups(root: LayoutNode | null): GroupNode[] {
  if (!root || root.kind === 'tile') return [];
  if (root.kind === 'group') return [root, ...groups(root.child)];
  return root.children.flatMap(groups);
}

/** Puts a frame around a single tile, starting a new group. */
export function groupTile(
  root: LayoutNode | null,
  sessionId: string,
  group: { id?: string; name: string; color: string },
): LayoutNode | null {
  if (!root || groupOf(root, tileKey(sessionId))) return root;
  return replaceNode(root, tileKey(sessionId), (child) => ({
    kind: 'group',
    id: group.id ?? newId(),
    name: group.name,
    color: group.color,
    child,
  }));
}

/** Removes a group's frame; its tiles stay where they are. */
export function dissolveGroup(root: LayoutNode | null, groupId: string): LayoutNode | null {
  if (!root) return root;
  return normalize(replaceNode(root, groupId, (node) => (node.kind === 'group' ? node.child : node)));
}

export function updateGroup(
  root: LayoutNode | null,
  groupId: string,
  patch: { name?: string; color?: string },
): LayoutNode | null {
  if (!root) return root;
  return replaceNode(root, groupId, (node) => (node.kind === 'group' ? { ...node, ...patch } : node));
}

/** Moves a tile into a group, after the group's last tile. */
export function addToGroup(root: LayoutNode | null, sessionId: string, groupId: string): LayoutNode | null {
  const target = findNode(root, groupId);
  if (!target || target.kind !== 'group' || groupOf(root, tileKey(sessionId))?.id === groupId) return root;
  const { root: without, removed } = removeTile(root, sessionId);
  const group = findNode(without, groupId);
  if (!removed || !group) return root;
  return insertNode(without, removed, tileKey(leaves(group).at(-1)!), 'right');
}

/** Moves a tile out of its group, next to the group. */
export function removeFromGroup(root: LayoutNode | null, sessionId: string): LayoutNode | null {
  const group = groupOf(root, tileKey(sessionId));
  if (!group) return root;
  const { root: without, removed } = removeTile(root, sessionId);
  if (!removed) return root;
  return findNode(without, group.id) ? insertNode(without, removed, group.id, 'right') : appendNode(without, removed);
}

export function setSplitSizes(root: LayoutNode | null, splitId: string, sizes: number[]): LayoutNode | null {
  if (!root) return root;
  return replaceNode(root, splitId, (node) =>
    node.kind === 'split' && sizes.length === node.children.length && sizes.every((s) => Number.isFinite(s) && s >= 0)
      ? { ...node, sizes: scaleTo100(sizes) }
      : node,
  );
}
