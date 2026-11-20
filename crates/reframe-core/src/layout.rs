//! Pure operations on a window's split layout.
//!
//! Ported from the Electron version's `src/shared/layout.ts` (its node types live in
//! [`crate::state`]). A layout is a tree of splits (row = side by side, col = stacked), groups (a
//! named, coloured frame around a subtree) and tiles (one session each). Every operation returns a
//! new, normalized tree: no split has fewer than two children, nested splits in the same direction
//! are flattened, and split sizes are percentages that sum to 100.
//!
//! Where the TypeScript takes `LayoutNode | null`, the functions here take `Option<&LayoutNode>`
//! and return an owned tree; a node that goes into the tree is passed by value.

use crate::state::{GroupNode, LayoutNode, RestoreHint, Side, SplitDir, SplitNode, TileNode};

/// Where a tile is dropped: next to another node, on one of its sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropTarget {
    /// Key of the target node, see [`node_key`].
    pub key: String,
    pub side: Side,
}

/// What [`group_tile`] frames a tile with; without an id the group gets a new one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewGroup {
    pub id: Option<String>,
    pub name: String,
    pub color: String,
}

/// What [`update_group`] changes; `None` leaves the value as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupPatch {
    pub name: Option<String>,
    pub color: Option<String>,
}

/// What [`remove_node`] returns: the remaining tree, the node taken out, and where it sat.
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    pub root: Option<LayoutNode>,
    pub removed: Option<LayoutNode>,
    pub hint: Option<RestoreHint>,
}

/// The side a node is inserted on when none is given.
pub const DEFAULT_SIDE: Side = Side::Right;
/// The share of the anchor's slot a node gets when none is given.
pub const DEFAULT_RATIO: f64 = 0.5;

const TILE_KEY_PREFIX: &str = "tile:";

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A tile's node key: `tile:<session id>`. Persisted in restore hints.
pub fn tile_key(session_id: &str) -> String {
    format!("{TILE_KEY_PREFIX}{session_id}")
}

/// A node's key: [`tile_key`] for a tile, the node's id otherwise.
pub fn node_key(node: &LayoutNode) -> String {
    match node {
        LayoutNode::Tile(tile) => tile_key(&tile.session_id),
        LayoutNode::Split(split) => split.id.clone(),
        LayoutNode::Group(group) => group.id.clone(),
    }
}

/// `node_key(node) == key`, without building the key.
fn has_key(node: &LayoutNode, key: &str) -> bool {
    match node {
        LayoutNode::Tile(tile) => key.strip_prefix(TILE_KEY_PREFIX) == Some(tile.session_id.as_str()),
        LayoutNode::Split(split) => split.id == key,
        LayoutNode::Group(group) => group.id == key,
    }
}

pub fn tile(session_id: impl Into<String>) -> LayoutNode {
    LayoutNode::Tile(TileNode {
        session_id: session_id.into(),
    })
}

/// The sessions in the tree, in the order of their tiles.
pub fn leaves(node: Option<&LayoutNode>) -> Vec<String> {
    fn collect(node: &LayoutNode, into: &mut Vec<String>) {
        match node {
            LayoutNode::Tile(tile) => into.push(tile.session_id.clone()),
            LayoutNode::Group(group) => collect(&group.child, into),
            LayoutNode::Split(split) => split.children.iter().for_each(|child| collect(child, into)),
        }
    }
    let mut sessions = Vec::new();
    if let Some(node) = node {
        collect(node, &mut sessions);
    }
    sessions
}

pub fn find_node<'a>(node: Option<&'a LayoutNode>, key: &str) -> Option<&'a LayoutNode> {
    let node = node?;
    if has_key(node, key) {
        return Some(node);
    }
    match node {
        LayoutNode::Tile(_) => None,
        LayoutNode::Group(group) => find_node(Some(&group.child), key),
        LayoutNode::Split(split) => split.children.iter().find_map(|child| find_node(Some(child), key)),
    }
}

fn side_dir(side: Side) -> SplitDir {
    match side {
        Side::Left | Side::Right => SplitDir::Row,
        Side::Top | Side::Bottom => SplitDir::Col,
    }
}

fn side_first(side: Side) -> bool {
    matches!(side, Side::Left | Side::Top)
}

fn sum(sizes: &[f64]) -> f64 {
    sizes.iter().fold(0.0, |a, b| a + b)
}

fn scale_to_100(sizes: &[f64]) -> Vec<f64> {
    let total = sum(sizes);
    // Not `total <= 0.0`: a total that is not a number gives equal sizes too.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(total > 0.0) {
        return vec![100.0 / sizes.len() as f64; sizes.len()];
    }
    sizes.iter().map(|size| (size / total) * 100.0).collect()
}

pub fn normalize(node: LayoutNode) -> LayoutNode {
    let split = match node {
        LayoutNode::Tile(_) => return node,
        LayoutNode::Group(mut group) => {
            group.child = Box::new(normalize(*group.child));
            return LayoutNode::Group(group);
        }
        LayoutNode::Split(split) => split,
    };
    let SplitNode {
        id,
        dir,
        children: raw,
        sizes,
    } = split;
    let sizes = if sizes.len() == raw.len() {
        sizes
    } else {
        vec![1.0; raw.len()]
    };
    let mut children = Vec::with_capacity(raw.len());
    let mut child_sizes = Vec::with_capacity(raw.len());
    for (raw_child, size) in raw.into_iter().zip(sizes) {
        match normalize(raw_child) {
            LayoutNode::Split(child) if child.dir == dir => {
                let inner = scale_to_100(&child.sizes);
                for (j, grandchild) in child.children.into_iter().enumerate() {
                    children.push(grandchild);
                    child_sizes.push((size * inner.get(j).copied().unwrap_or(0.0)) / 100.0);
                }
            }
            child => {
                children.push(child);
                child_sizes.push(size);
            }
        }
    }
    if children.len() == 1
        && let Some(only) = children.pop()
    {
        return only;
    }
    LayoutNode::Split(SplitNode {
        id,
        dir,
        children,
        sizes: scale_to_100(&child_sizes),
    })
}

/// Replaces every node with `key` by what `replace` makes of it (it does not look into the node
/// it replaced).
fn replace_node(node: LayoutNode, key: &str, replace: &mut dyn FnMut(LayoutNode) -> LayoutNode) -> LayoutNode {
    if has_key(&node, key) {
        return replace(node);
    }
    match node {
        LayoutNode::Tile(_) => node,
        LayoutNode::Group(mut group) => {
            group.child = Box::new(replace_node(*group.child, key, replace));
            LayoutNode::Group(group)
        }
        LayoutNode::Split(mut split) => {
            split.children = split
                .children
                .into_iter()
                .map(|child| replace_node(child, key, replace))
                .collect();
            LayoutNode::Split(split)
        }
    }
}

/// Puts `node` next to the node with key `anchor` on `side`; `ratio` is the new node's share
/// (0..1). The TypeScript's defaults are [`DEFAULT_SIDE`] and [`DEFAULT_RATIO`].
pub fn insert_node(
    root: Option<&LayoutNode>,
    node: LayoutNode,
    anchor: Option<&str>,
    side: Side,
    ratio: f64,
) -> LayoutNode {
    let Some(root_node) = root else { return node };
    // As in JavaScript, where an empty anchor counts as none.
    let anchor = anchor.filter(|anchor| !anchor.is_empty());
    let Some(anchor) = anchor.filter(|anchor| find_node(root, anchor).is_some()) else {
        return append_node(root, node);
    };
    let r = ratio.clamp(0.1, 0.9);
    let mut wrap = |existing: LayoutNode| {
        let first = side_first(side);
        let node = node.clone();
        LayoutNode::Split(SplitNode {
            id: new_id(),
            dir: side_dir(side),
            children: if first {
                vec![node, existing]
            } else {
                vec![existing, node]
            },
            sizes: if first {
                vec![r * 100.0, (1.0 - r) * 100.0]
            } else {
                vec![(1.0 - r) * 100.0, r * 100.0]
            },
        })
    };
    normalize(replace_node(root_node.clone(), anchor, &mut wrap))
}

/// Adds a node where new tiles go when no place is given: the first fills the window, the second
/// comes to its right (half and half), and every further one at the bottom of the column with the
/// fewest tiles (the right one when they are even), so two columns fill in turn: 3 below 2, 4
/// below 1, 5 below 3. That rule also holds for a layout the user built (with more columns, or a
/// single one, which gets a second); a group in the chosen column gets the node below it, not
/// into it.
pub fn append_node(root: Option<&LayoutNode>, node: LayoutNode) -> LayoutNode {
    let Some(root) = root else { return node };
    if let LayoutNode::Split(row) = root
        && row.dir == SplitDir::Row
    {
        let count = |column: &LayoutNode| match column {
            LayoutNode::Split(split) if split.dir == SplitDir::Col => split.children.len(),
            _ => 1,
        };
        let mut target = row.children.len().saturating_sub(1);
        for (i, column) in row.children.iter().enumerate() {
            if row
                .children
                .get(target)
                .is_some_and(|fewest| count(column) < count(fewest))
            {
                target = i;
            }
        }
        let mut node = Some(node);
        let children = row
            .children
            .iter()
            .enumerate()
            .map(|(i, column)| match node.take_if(|_| i == target) {
                Some(node) => stack_below(column, node),
                None => column.clone(),
            })
            .collect();
        return normalize(LayoutNode::Split(SplitNode {
            id: row.id.clone(),
            dir: row.dir,
            children,
            sizes: row.sizes.clone(),
        }));
    }
    normalize(LayoutNode::Split(SplitNode {
        id: new_id(),
        dir: SplitDir::Row,
        children: vec![root.clone(), node],
        sizes: vec![50.0, 50.0],
    }))
}

/// `node` at the bottom of `column` (made one if needed); the heights there keep their
/// proportions.
fn stack_below(column: &LayoutNode, node: LayoutNode) -> LayoutNode {
    if let LayoutNode::Split(split) = column
        && split.dir == SplitDir::Col
    {
        let n = split.children.len() as f64;
        let mut sizes: Vec<f64> = scale_to_100(&split.sizes)
            .into_iter()
            .map(|size| (size * n) / (n + 1.0))
            .collect();
        sizes.push(100.0 / (n + 1.0));
        let mut children = split.children.clone();
        children.push(node);
        return normalize(LayoutNode::Split(SplitNode {
            id: split.id.clone(),
            dir: split.dir,
            children,
            sizes,
        }));
    }
    LayoutNode::Split(SplitNode {
        id: new_id(),
        dir: SplitDir::Col,
        children: vec![column.clone(), node],
        sizes: vec![50.0, 50.0],
    })
}

pub fn insert_tile(root: Option<&LayoutNode>, session_id: &str, target: Option<&DropTarget>) -> LayoutNode {
    insert_node(
        root,
        tile(session_id),
        target.map(|target| target.key.as_str()),
        target.map_or(DEFAULT_SIDE, |target| target.side),
        DEFAULT_RATIO,
    )
}

/// What the walk of [`remove_node`] has found so far.
#[derive(Default)]
struct Found {
    removed: Option<LayoutNode>,
    hint: Option<RestoreHint>,
}

fn remove_walk(node: &LayoutNode, key: &str, found: &mut Found) -> Option<LayoutNode> {
    let split = match node {
        LayoutNode::Tile(_) => return Some(node.clone()),
        LayoutNode::Group(group) => {
            if has_key(&group.child, key) {
                found.removed = Some((*group.child).clone());
                return None; // an empty group disappears
            }
            let child = remove_walk(&group.child, key, found)?;
            return Some(LayoutNode::Group(GroupNode {
                id: group.id.clone(),
                name: group.name.clone(),
                color: group.color.clone(),
                child: Box::new(child),
            }));
        }
        LayoutNode::Split(split) => split,
    };
    if let Some(index) = split.children.iter().position(|child| has_key(child, key)) {
        found.removed = split.children.get(index).cloned();
        let neighbor_index = if index > 0 { index - 1 } else { index + 1 };
        // A split always has a neighbour for its child; one with a single child (which no
        // operation here makes, and the state file does not allow) leaves the hint as it was.
        if let Some(neighbor) = split.children.get(neighbor_index) {
            let own = split.sizes.get(index).copied().unwrap_or(0.0);
            let other = split.sizes.get(neighbor_index).copied().unwrap_or(0.0);
            let before = neighbor_index < index;
            let total = sum(&split.sizes);
            found.hint = Some(RestoreHint {
                anchor: node_key(neighbor),
                side: match (split.dir, before) {
                    (SplitDir::Row, true) => Side::Right,
                    (SplitDir::Row, false) => Side::Left,
                    (SplitDir::Col, true) => Side::Bottom,
                    (SplitDir::Col, false) => Side::Top,
                },
                ratio: if own + other > 0.0 { own / (own + other) } else { 0.5 },
                share: (total > 0.0).then(|| own / total),
            });
        }
        let mut children = split.children.clone();
        children.remove(index);
        let mut sizes = split.sizes.clone();
        if index < sizes.len() {
            sizes.remove(index);
        }
        return Some(LayoutNode::Split(SplitNode {
            id: split.id.clone(),
            dir: split.dir,
            children,
            sizes,
        }));
    }
    let mut children = Vec::with_capacity(split.children.len());
    let mut sizes = Vec::with_capacity(split.children.len());
    for (i, child) in split.children.iter().enumerate() {
        if let Some(next) = remove_walk(child, key, found) {
            children.push(next);
            sizes.push(split.sizes.get(i).copied().unwrap_or(0.0));
        }
    }
    if children.is_empty() {
        return None;
    }
    Some(LayoutNode::Split(SplitNode {
        id: split.id.clone(),
        dir: split.dir,
        children,
        sizes,
    }))
}

/// Removes the node with `key` and returns the remaining tree plus a hint for putting it back.
pub fn remove_node(root: Option<&LayoutNode>, key: &str) -> Removal {
    let Some(root) = root else {
        return Removal {
            root: None,
            removed: None,
            hint: None,
        };
    };
    if has_key(root, key) {
        return Removal {
            root: None,
            removed: Some(root.clone()),
            hint: None,
        };
    }
    let mut found = Found::default();
    let next = remove_walk(root, key, &mut found);
    Removal {
        root: next.map(normalize),
        removed: found.removed,
        hint: found.hint,
    }
}

pub fn remove_tile(root: Option<&LayoutNode>, session_id: &str) -> Removal {
    remove_node(root, &tile_key(session_id))
}

/// Puts a node back where a [`RestoreHint`] says; falls back to appending if the anchor is gone.
pub fn restore_node(root: Option<&LayoutNode>, node: LayoutNode, hint: Option<&RestoreHint>) -> LayoutNode {
    let Some(hint) = hint.filter(|hint| find_node(root, &hint.anchor).is_some()) else {
        return append_node(root, node);
    };
    let key = node_key(&node);
    let restored = insert_node(root, node, Some(&hint.anchor), hint.side, hint.ratio);
    let (Some(parent), Some(share)) = (find_parent_split(&restored, &key), hint.share) else {
        return restored;
    };
    if parent.dir != side_dir(hint.side) || !(share > 0.0 && share < 1.0) {
        return restored;
    }
    let Some(index) = parent.children.iter().position(|child| has_key(child, &key)) else {
        return restored;
    };
    // Take the remembered share back from all siblings, as they all grew when the tile left.
    // Insertion carved the tile out of the anchor's slot, so the anchor gets that space back
    // before scaling.
    let anchor_index = parent.children.iter().position(|child| has_key(child, &hint.anchor));
    let own = parent.sizes.get(index).copied().unwrap_or(0.0);
    let before = parent
        .sizes
        .iter()
        .enumerate()
        .map(|(i, size)| if Some(i) == anchor_index { size + own } else { *size });
    let others: Vec<f64> = before
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, s)| s)
        .collect();
    let mut others = scale_to_100(&others).into_iter();
    let sizes: Vec<f64> = (0..parent.children.len())
        .map(|i| {
            if i == index {
                share * 100.0
            } else {
                others.next().unwrap_or(0.0) * (1.0 - share)
            }
        })
        .collect();
    let parent_id = parent.id.clone();
    set_split_sizes(Some(&restored), &parent_id, &sizes).unwrap_or(restored)
}

fn find_parent_split<'a>(node: &'a LayoutNode, key: &str) -> Option<&'a SplitNode> {
    match node {
        LayoutNode::Tile(_) => None,
        LayoutNode::Group(group) => find_parent_split(&group.child, key),
        LayoutNode::Split(split) => {
            if split.children.iter().any(|child| has_key(child, key)) {
                return Some(split);
            }
            split.children.iter().find_map(|child| find_parent_split(child, key))
        }
    }
}

/// Moves the node with `key` (a tile or a whole group) next to the target.
pub fn move_node(root: Option<&LayoutNode>, key: &str, target: &DropTarget) -> Option<LayoutNode> {
    // Dropping a node onto itself or a group onto one of its own tiles changes nothing.
    let Some(node) = find_node(root, key) else {
        return root.cloned();
    };
    if find_node(Some(node), &target.key).is_some() {
        return root.cloned();
    }
    let Removal {
        root: without, removed, ..
    } = remove_node(root, key);
    let Some(removed) = removed else { return root.cloned() };
    let resolved = resolve_target(without.as_ref(), &removed, target);
    Some(insert_node(
        without.as_ref(),
        removed,
        Some(&resolved.key),
        resolved.side,
        DEFAULT_RATIO,
    ))
}

pub fn move_tile(root: Option<&LayoutNode>, session_id: &str, target: &DropTarget) -> Option<LayoutNode> {
    move_node(root, &tile_key(session_id), target)
}

/// Groups do not nest: a group dropped next to a grouped tile lands next to that tile's group
/// instead.
pub fn resolve_target(root: Option<&LayoutNode>, node: &LayoutNode, target: &DropTarget) -> DropTarget {
    if !matches!(node, LayoutNode::Group(_)) {
        return target.clone();
    }
    match group_of(root, &target.key) {
        Some(group) => DropTarget {
            key: group.id.clone(),
            side: target.side,
        },
        None => target.clone(),
    }
}

// ---------- groups ----------

/// The group the node with `key` sits in, if any.
pub fn group_of<'a>(root: Option<&'a LayoutNode>, key: &str) -> Option<&'a GroupNode> {
    // The outer `Option`: whether the node was found at all.
    fn walk<'a>(node: &'a LayoutNode, key: &str, group: Option<&'a GroupNode>) -> Option<Option<&'a GroupNode>> {
        if has_key(node, key) {
            return Some(group);
        }
        match node {
            LayoutNode::Tile(_) => None,
            LayoutNode::Group(inner) => walk(&inner.child, key, Some(inner)),
            LayoutNode::Split(split) => split.children.iter().find_map(|child| walk(child, key, group)),
        }
    }
    root.and_then(|root| walk(root, key, None)).flatten()
}

pub fn groups(root: Option<&LayoutNode>) -> Vec<&GroupNode> {
    match root {
        None | Some(LayoutNode::Tile(_)) => Vec::new(),
        Some(LayoutNode::Group(group)) => {
            let mut all = vec![group];
            all.extend(groups(Some(&group.child)));
            all
        }
        Some(LayoutNode::Split(split)) => split.children.iter().flat_map(|child| groups(Some(child))).collect(),
    }
}

/// Puts a frame around a single tile, starting a new group.
pub fn group_tile(root: Option<&LayoutNode>, session_id: &str, group: NewGroup) -> Option<LayoutNode> {
    let key = tile_key(session_id);
    let root = root?;
    if group_of(Some(root), &key).is_some() {
        return Some(root.clone());
    }
    let mut frame = |child: LayoutNode| {
        LayoutNode::Group(GroupNode {
            id: group.id.clone().unwrap_or_else(new_id),
            name: group.name.clone(),
            color: group.color.clone(),
            child: Box::new(child),
        })
    };
    Some(replace_node(root.clone(), &key, &mut frame))
}

/// Removes a group's frame; its tiles stay where they are.
pub fn dissolve_group(root: Option<&LayoutNode>, group_id: &str) -> Option<LayoutNode> {
    let mut unframe = |node: LayoutNode| match node {
        LayoutNode::Group(group) => *group.child,
        other => other,
    };
    Some(normalize(replace_node(root?.clone(), group_id, &mut unframe)))
}

pub fn update_group(root: Option<&LayoutNode>, group_id: &str, patch: &GroupPatch) -> Option<LayoutNode> {
    let mut apply = |node: LayoutNode| match node {
        LayoutNode::Group(mut group) => {
            if let Some(name) = &patch.name {
                group.name = name.clone();
            }
            if let Some(color) = &patch.color {
                group.color = color.clone();
            }
            LayoutNode::Group(group)
        }
        other => other,
    };
    Some(replace_node(root?.clone(), group_id, &mut apply))
}

/// Moves a tile into a group. A tile right next to the group keeps its place and the frame grows
/// around it; any other tile goes after the group's last tile.
pub fn add_to_group(root: Option<&LayoutNode>, session_id: &str, group_id: &str) -> Option<LayoutNode> {
    let root_node = root?;
    let unchanged = || Some(root_node.clone());
    if !matches!(find_node(root, group_id), Some(LayoutNode::Group(_)))
        || group_of(root, &tile_key(session_id)).is_some_and(|group| group.id == group_id)
    {
        return unchanged();
    }
    if let Some(grown) = grow_around(root_node, session_id, group_id) {
        return Some(grown);
    }
    let Removal {
        root: without, removed, ..
    } = remove_tile(root, session_id);
    let (Some(removed), Some(group)) = (removed, find_node(without.as_ref(), group_id)) else {
        return unchanged();
    };
    let last = leaves(Some(group)).pop().map(|session| tile_key(&session));
    Some(insert_node(
        without.as_ref(),
        removed,
        last.as_deref(),
        Side::Right,
        DEFAULT_RATIO,
    ))
}

/// Widens the group's frame over the tile when both sit side by side in the same split (the tile
/// alone or in a group of its own, which disappears), so nothing moves; `None` when they are not
/// neighbours.
fn grow_around(root: &LayoutNode, session_id: &str, group_id: &str) -> Option<LayoutNode> {
    let parent = find_parent_split(root, group_id)?;
    let index = parent.children.iter().position(|child| has_key(child, group_id))?;
    let key = tile_key(session_id);
    let is_tile = |node: &LayoutNode| {
        has_key(node, &key) || matches!(node, LayoutNode::Group(group) if has_key(&group.child, &key))
    };
    let other = [index.checked_sub(1), index.checked_add(1)]
        .into_iter()
        .flatten()
        .find(|i| parent.children.get(*i).is_some_and(is_tile))?;
    let LayoutNode::Group(group) = parent.children.get(index)? else {
        return None;
    };
    let before = other < index;
    let own = parent.sizes.get(index).copied().unwrap_or(0.0);
    let theirs = parent.sizes.get(other).copied().unwrap_or(0.0);
    let inside = (*group.child).clone();
    let grown = LayoutNode::Group(GroupNode {
        id: group.id.clone(),
        name: group.name.clone(),
        color: group.color.clone(),
        child: Box::new(LayoutNode::Split(SplitNode {
            id: new_id(),
            dir: parent.dir,
            children: if before {
                vec![tile(session_id), inside]
            } else {
                vec![inside, tile(session_id)]
            },
            sizes: if before { vec![theirs, own] } else { vec![own, theirs] },
        })),
    });
    let first = index.min(other);
    let mut children = parent.children.clone();
    children.splice(first..first + 2, [grown]);
    // Like JavaScript's `splice(first, 2, …)`, also where there are fewer sizes than children.
    let mut sizes = parent.sizes.clone();
    let from = first.min(sizes.len());
    let to = (first + 2).min(sizes.len());
    sizes.splice(from..to, [own + theirs]);
    let widened = LayoutNode::Split(SplitNode {
        id: parent.id.clone(),
        dir: parent.dir,
        children,
        sizes,
    });
    Some(normalize(replace_node(root.clone(), &parent.id, &mut |_| {
        widened.clone()
    })))
}

/// Moves a tile out of its group, next to the group.
pub fn remove_from_group(root: Option<&LayoutNode>, session_id: &str) -> Option<LayoutNode> {
    let Some(group) = group_of(root, &tile_key(session_id)) else {
        return root.cloned();
    };
    let Removal {
        root: without, removed, ..
    } = remove_tile(root, session_id);
    let Some(removed) = removed else { return root.cloned() };
    Some(if find_node(without.as_ref(), &group.id).is_some() {
        insert_node(without.as_ref(), removed, Some(&group.id), Side::Right, DEFAULT_RATIO)
    } else {
        append_node(without.as_ref(), removed)
    })
}

/// Sets a split's sizes (scaled to sum to 100); sizes that do not fit it are ignored.
pub fn set_split_sizes(root: Option<&LayoutNode>, split_id: &str, sizes: &[f64]) -> Option<LayoutNode> {
    let mut resize = |node: LayoutNode| match node {
        LayoutNode::Split(mut split)
            if sizes.len() == split.children.len() && sizes.iter().all(|size| size.is_finite() && *size >= 0.0) =>
        {
            split.sizes = scale_to_100(sizes);
            LayoutNode::Split(split)
        }
        other => other,
    };
    Some(replace_node(root?.clone(), split_id, &mut resize))
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
