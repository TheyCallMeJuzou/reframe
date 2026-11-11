//! Layout intents a window can send, and the pure reducer applied to its [`WindowState`].
//!
//! Ported from the Electron version's `src/shared/actions.ts`.
// `isDropTarget` and `isLayoutAction` are not ported: they checked untyped JSON from IPC, which the types do here.

use crate::layout::{
    DEFAULT_RATIO, DropTarget, GroupPatch, Removal, append_node, find_node, group_of, groups, insert_node, insert_tile,
    leaves, move_node, remove_node, remove_tile, resolve_target, restore_node, set_split_sizes, tile, tile_key,
    update_group,
};
use crate::state::{DockEntry, GroupNode, LayoutNode, WindowState};

#[derive(Clone, Debug, PartialEq)]
pub enum LayoutAction {
    /// Moves a tile or a whole group (by node key) next to a target in the same window. A
    /// collapsed one (dragged out of the session list) comes out of the side bar, to the end of
    /// the layout without a target.
    NodeMove {
        key: String,
        target: Option<DropTarget>,
    },
    TileCollapse {
        session_id: String,
    },
    TileExpand {
        session_id: String,
    },
    GroupCollapse {
        group_id: String,
    },
    GroupExpand {
        group_id: String,
    },
    GroupRename {
        group_id: String,
        name: String,
    },
    SplitResize {
        split_id: String,
        sizes: Vec<f64>,
    },
    WindowSidebar {
        open: bool,
    },
}

/// What [`take_node`] returns: the window without the node, and the node (if it was there).
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    pub win: WindowState,
    pub node: Option<LayoutNode>,
}

/// The longest group name, in UTF-16 code units as JavaScript counts them.
const GROUP_NAME_MAX: usize = 80;

pub fn dock_sessions(entry: &DockEntry) -> Vec<String> {
    match entry {
        DockEntry::Tile { session_id, .. } => vec![session_id.clone()],
        DockEntry::Group { node, .. } => leaves(Some(&node.child)),
    }
}

/// Every session in the window, visible or collapsed.
pub fn window_sessions(win: &WindowState) -> Vec<String> {
    let mut sessions = leaves(win.layout.as_ref());
    sessions.extend(win.dock.iter().flat_map(dock_sessions));
    sessions
}

pub fn window_has_session(win: &WindowState, session_id: &str) -> bool {
    window_sessions(win).iter().any(|id| id == session_id)
}

/// Whether the session is collapsed into the side bar (alone or with its group).
pub fn is_collapsed(win: &WindowState, session_id: &str) -> bool {
    win.dock
        .iter()
        .any(|entry| dock_sessions(entry).iter().any(|id| id == session_id))
}

/// The dock without the entry at `index`.
fn dock_without(dock: &[DockEntry], index: usize) -> Vec<DockEntry> {
    let mut others = dock.to_vec();
    others.remove(index);
    others
}

pub fn apply_layout_action(win: &WindowState, action: &LayoutAction) -> WindowState {
    let layout = win.layout.as_ref();
    match action {
        LayoutAction::NodeMove { key, target } => {
            if find_node(layout, key).is_none() {
                let Taken { win: rest, node } = take_node(win, key);
                return match node {
                    Some(node) => put_node(&rest, node, target.as_ref()),
                    None => win.clone(),
                };
            }
            match target {
                Some(target) => WindowState {
                    layout: move_node(layout, key, target),
                    ..win.clone()
                },
                None => win.clone(),
            }
        }
        LayoutAction::TileCollapse { session_id } => {
            // The last tile of a group takes its group along, otherwise the group would be lost.
            if let Some(group) = group_of(layout, &tile_key(session_id))
                && leaves(Some(&group.child)).len() == 1
            {
                let group_id = group.id.clone();
                return apply_layout_action(win, &LayoutAction::GroupCollapse { group_id });
            }
            let Removal { root, removed, hint } = remove_tile(layout, session_id);
            if removed.is_none() {
                return win.clone();
            }
            let mut dock = win.dock.clone();
            dock.push(DockEntry::Tile {
                session_id: session_id.clone(),
                hint,
            });
            WindowState {
                layout: root,
                dock,
                ..win.clone()
            }
        }
        LayoutAction::TileExpand { session_id } => {
            let found = win.dock.iter().enumerate().find_map(|(index, entry)| match entry {
                DockEntry::Tile { session_id: id, hint } if id == session_id => Some((index, hint)),
                _ => None,
            });
            let Some((index, hint)) = found else { return win.clone() };
            WindowState {
                layout: Some(restore_node(layout, tile(session_id.as_str()), hint.as_ref())),
                dock: dock_without(&win.dock, index),
                ..win.clone()
            }
        }
        LayoutAction::GroupCollapse { group_id } => {
            let Removal { root, removed, hint } = remove_node(layout, group_id);
            let Some(LayoutNode::Group(node)) = removed else {
                return win.clone();
            };
            let mut dock = win.dock.clone();
            dock.push(DockEntry::Group { node, hint });
            WindowState {
                layout: root,
                dock,
                ..win.clone()
            }
        }
        LayoutAction::GroupExpand { group_id } => {
            let found = win.dock.iter().enumerate().find_map(|(index, entry)| match entry {
                DockEntry::Group { node, hint } if node.id == *group_id => Some((index, node, hint)),
                _ => None,
            });
            let Some((index, node, hint)) = found else {
                return win.clone();
            };
            WindowState {
                layout: Some(restore_node(layout, LayoutNode::Group(node.clone()), hint.as_ref())),
                dock: dock_without(&win.dock, index),
                ..win.clone()
            }
        }
        LayoutAction::GroupRename { group_id, name } => {
            let name = first_utf16_units(js_trim(name), GROUP_NAME_MAX);
            if name.is_empty() {
                return win.clone();
            }
            let patch = GroupPatch {
                name: Some(name.to_owned()),
                color: None,
            };
            patch_group(win, group_id, &patch)
        }
        LayoutAction::SplitResize { split_id, sizes } => WindowState {
            layout: set_split_sizes(layout, split_id, sizes),
            ..win.clone()
        },
        LayoutAction::WindowSidebar { open } => WindowState {
            sidebar_open: Some(*open),
            ..win.clone()
        },
    }
}

/// `String.prototype.trim`: JavaScript's white space and line ends, which are not quite Rust's
/// (U+FEFF counts, U+0085 does not).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c| {
        matches!(
            c,
            '\u{9}'..='\u{D}'
                | ' '
                | '\u{A0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
        )
    })
}

/// `text.slice(0, max)` in JavaScript's UTF-16 code units. A character outside the BMP that
/// would be cut in half there is left out whole here, as a string cannot hold half of one.
fn first_utf16_units(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &text[..at];
        }
    }
    text
}

/// Changes a group's name or color, whether it is visible or collapsed.
pub fn patch_group(win: &WindowState, group_id: &str, patch: &GroupPatch) -> WindowState {
    let dock = win
        .dock
        .iter()
        .map(|entry| match entry {
            DockEntry::Group { node, hint } if node.id == group_id => DockEntry::Group {
                node: GroupNode {
                    id: node.id.clone(),
                    name: patch.name.clone().unwrap_or_else(|| node.name.clone()),
                    color: patch.color.clone().unwrap_or_else(|| node.color.clone()),
                    child: node.child.clone(),
                },
                hint: hint.clone(),
            },
            other => other.clone(),
        })
        .collect();
    WindowState {
        layout: update_group(win.layout.as_ref(), group_id, patch),
        dock,
        ..win.clone()
    }
}

/// Adds a new session's tile; with a target it lands next to that node, otherwise at the end.
pub fn add_tile(win: &WindowState, session_id: &str, target: Option<&DropTarget>) -> WindowState {
    WindowState {
        layout: Some(insert_tile(win.layout.as_ref(), session_id, target)),
        ..win.clone()
    }
}

/// Removes a session from the window entirely (layout, collapsed tiles and collapsed groups).
pub fn drop_session(win: &WindowState, session_id: &str) -> WindowState {
    let mut dock = Vec::with_capacity(win.dock.len());
    for entry in &win.dock {
        match entry {
            DockEntry::Tile { session_id: id, .. } => {
                if id != session_id {
                    dock.push(entry.clone());
                }
            }
            DockEntry::Group { node, hint } => {
                let rest = remove_tile(Some(&LayoutNode::Group(node.clone())), session_id).root;
                if let Some(LayoutNode::Group(rest)) = rest {
                    dock.push(DockEntry::Group {
                        node: rest,
                        hint: hint.clone(),
                    });
                }
            }
        }
    }
    WindowState {
        layout: remove_tile(win.layout.as_ref(), session_id).root,
        dock,
        ..win.clone()
    }
}

/// Takes a tile or group (by node key) out of a window, e.g. to move it to another window: out of
/// its layout, or out of the side bar (a tile of a collapsed group comes out alone, and the group
/// stays collapsed with the rest).
pub fn take_node(win: &WindowState, key: &str) -> Taken {
    let Removal { root, removed, .. } = remove_node(win.layout.as_ref(), key);
    if let Some(node) = removed {
        let win = WindowState {
            layout: root,
            ..win.clone()
        };
        return Taken { win, node: Some(node) };
    }
    let with_dock = |dock: Vec<DockEntry>| WindowState { dock, ..win.clone() };
    for (index, entry) in win.dock.iter().enumerate() {
        match entry {
            DockEntry::Tile { session_id, .. } => {
                if tile_key(session_id) == key {
                    return Taken {
                        win: with_dock(dock_without(&win.dock, index)),
                        node: Some(tile(session_id.as_str())),
                    };
                }
            }
            DockEntry::Group { node, hint } => {
                if node.id == key {
                    return Taken {
                        win: with_dock(dock_without(&win.dock, index)),
                        node: Some(LayoutNode::Group(node.clone())),
                    };
                }
                let in_group = remove_node(Some(&LayoutNode::Group(node.clone())), key);
                let Some(removed) = in_group.removed else { continue };
                let dock = match in_group.root {
                    Some(LayoutNode::Group(rest)) => {
                        let mut dock = win.dock.clone();
                        if let Some(slot) = dock.get_mut(index) {
                            *slot = DockEntry::Group {
                                node: rest,
                                hint: hint.clone(),
                            };
                        }
                        dock
                    }
                    _ => dock_without(&win.dock, index),
                };
                return Taken {
                    win: with_dock(dock),
                    node: Some(removed),
                };
            }
        }
    }
    Taken {
        win: win.clone(),
        node: None,
    }
}

/// Puts a tile or group into a window next to a target, or at the end without one.
pub fn put_node(win: &WindowState, node: LayoutNode, target: Option<&DropTarget>) -> WindowState {
    let layout = win.layout.as_ref();
    let layout = match target.filter(|target| find_node(layout, &target.key).is_some()) {
        None => append_node(layout, node),
        Some(target) => {
            let resolved = resolve_target(layout, &node, target);
            insert_node(layout, node, Some(&resolved.key), resolved.side, DEFAULT_RATIO)
        }
    };
    WindowState {
        layout: Some(layout),
        ..win.clone()
    }
}

/// Every group in the window, visible or collapsed.
pub fn window_groups(win: &WindowState) -> Vec<&GroupNode> {
    let mut all = groups(win.layout.as_ref());
    all.extend(win.dock.iter().filter_map(|entry| match entry {
        DockEntry::Group { node, .. } => Some(node),
        DockEntry::Tile { .. } => None,
    }));
    all
}

/// A window with neither tiles nor collapsed tiles.
pub fn is_window_empty(win: &WindowState) -> bool {
    win.layout.is_none() && win.dock.is_empty()
}

/// The windows a start brings back: the ones with something in them, or the first one when all
/// are empty.
pub fn windows_to_restore(windows: &[WindowState]) -> Vec<WindowState> {
    let kept: Vec<WindowState> = windows.iter().filter(|win| !is_window_empty(win)).cloned().collect();
    if kept.is_empty() {
        windows.iter().take(1).cloned().collect()
    } else {
        kept
    }
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
