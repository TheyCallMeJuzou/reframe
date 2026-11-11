//! Workspaces: a window saved under a name, to be opened again later exactly like that, as a
//! window of its own.
//!
//! Ported from the Electron version's `src/shared/workspaces.ts`.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::actions::window_sessions;
use crate::layout::tile_key;
use crate::state::{
    DockEntry, GroupNode, LayoutNode, RestoreHint, SessionRecord, SplitNode, TileNode, WindowState, Workspace,
    WorkspaceWindow,
};

/// A snapshot of a window: its tiles, groups, collapsed tiles and size, and the sessions in it.
pub fn snapshot_window(
    win: &WindowState,
    sessions: &IndexMap<String, SessionRecord>,
    id: impl Into<String>,
    name: impl Into<String>,
    saved_at: f64,
) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        saved_at,
        window: WorkspaceWindow {
            bounds: win.bounds,
            maximized: win.maximized,
            sidebar_open: win.sidebar_open,
            layout: win.layout.clone(),
            dock: win.dock.clone(),
        },
        sessions: window_sessions(win)
            .iter()
            .filter_map(|id| sessions.get(id).cloned())
            .collect(),
    }
}

/// Renames what a saved window refers to, and remembers the node keys that changed with it.
struct Remap<'a, F: FnMut() -> String> {
    session_ids: &'a HashMap<String, String>,
    group_ids_in_use: &'a HashSet<String>,
    new_id: F,
    /// Saved node key → current node key.
    keys: HashMap<String, String>,
}

impl<F: FnMut() -> String> Remap<'_, F> {
    fn session(&self, id: &str) -> String {
        self.session_ids.get(id).map_or(id, String::as_str).to_owned()
    }

    fn node(&mut self, node: &LayoutNode) -> LayoutNode {
        match node {
            LayoutNode::Tile(tile) => LayoutNode::Tile(TileNode {
                session_id: self.session(&tile.session_id),
            }),
            LayoutNode::Group(group) => LayoutNode::Group(self.group(group)),
            LayoutNode::Split(split) => LayoutNode::Split(SplitNode {
                id: split.id.clone(),
                dir: split.dir,
                children: split.children.iter().map(|child| self.node(child)).collect(),
                sizes: split.sizes.clone(),
            }),
        }
    }

    fn group(&mut self, group: &GroupNode) -> GroupNode {
        let id = if self.group_ids_in_use.contains(&group.id) {
            (self.new_id)()
        } else {
            group.id.clone()
        };
        self.keys.insert(group.id.clone(), id.clone());
        GroupNode {
            id,
            name: group.name.clone(),
            color: group.color.clone(),
            child: Box::new(self.node(&group.child)),
        }
    }

    fn hint(&self, hint: Option<RestoreHint>) -> Option<RestoreHint> {
        hint.map(|hint| RestoreHint {
            anchor: self.keys.get(&hint.anchor).cloned().unwrap_or(hint.anchor),
            ..hint
        })
    }
}

/// The window that opens a workspace. Its tiles point to the sessions as reframe has them now
/// (`session_ids`: saved id → current id), and a group whose id is in use in another window gets
/// a new one, so groups stay unique.
pub fn window_from_workspace(
    workspace: &Workspace,
    window_id: &str,
    session_ids: &HashMap<String, String>,
    group_ids_in_use: &HashSet<String>,
    new_id: impl FnMut() -> String,
) -> WindowState {
    // Restore hints name a collapsed tile's neighbour by its key, so renamed keys are followed
    // there too.
    let keys = session_ids
        .iter()
        .map(|(from, to)| (tile_key(from), tile_key(to)))
        .collect();
    let mut remap = Remap {
        session_ids,
        group_ids_in_use,
        new_id,
        keys,
    };
    let layout = workspace.window.layout.as_ref().map(|layout| remap.node(layout));
    let dock: Vec<DockEntry> = workspace
        .window
        .dock
        .iter()
        .map(|entry| match entry {
            DockEntry::Tile { session_id, hint } => DockEntry::Tile {
                session_id: remap.session(session_id),
                hint: hint.clone(),
            },
            DockEntry::Group { node, hint } => DockEntry::Group {
                node: remap.group(node),
                hint: hint.clone(),
            },
        })
        .collect();
    // Only now, when every group has its id.
    let dock = dock
        .into_iter()
        .map(|entry| match entry {
            DockEntry::Tile { session_id, hint } => DockEntry::Tile {
                session_id,
                hint: remap.hint(hint),
            },
            DockEntry::Group { node, hint } => DockEntry::Group {
                node,
                hint: remap.hint(hint),
            },
        })
        .collect();
    WindowState {
        id: window_id.to_owned(),
        bounds: workspace.window.bounds,
        maximized: workspace.window.maximized,
        fullscreen: None,
        hidden: false,
        layout,
        dock,
        sidebar_open: workspace.window.sidebar_open,
        workspace_id: Some(workspace.id.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::tile;
    use crate::state::{Bounds, LaunchOptions, Lifecycle, Side, SplitDir};

    fn record(id: &str) -> SessionRecord {
        SessionRecord {
            id: id.into(),
            claude_session_id: format!("claude-{id}"),
            cwd: "/work".into(),
            name: None,
            claude_name: None,
            launch: LaunchOptions::default(),
            lifecycle: Lifecycle::Open,
            created_at: 0.0,
            last_active_at: 0.0,
            pid: None,
            linux_pid: None,
            used: None,
        }
    }

    fn records(ids: &[&str]) -> IndexMap<String, SessionRecord> {
        ids.iter().map(|id| ((*id).to_owned(), record(id))).collect()
    }

    fn grouped() -> LayoutNode {
        LayoutNode::Split(SplitNode {
            id: "s1".into(),
            dir: SplitDir::Row,
            sizes: vec![50.0, 50.0],
            children: vec![
                tile("a"),
                LayoutNode::Group(GroupNode {
                    id: "g1".into(),
                    name: "Group 1".into(),
                    color: "#d97757".into(),
                    child: Box::new(tile("b")),
                }),
            ],
        })
    }

    const BOUNDS: Bounds = Bounds {
        x: 10.0,
        y: 20.0,
        width: 900.0,
        height: 600.0,
    };

    fn hint(anchor: &str) -> Option<RestoreHint> {
        Some(RestoreHint {
            anchor: anchor.into(),
            side: Side::Right,
            ratio: 0.5,
            share: None,
        })
    }

    fn saved() -> WindowState {
        WindowState {
            bounds: Some(BOUNDS),
            layout: Some(grouped()),
            dock: vec![DockEntry::Tile {
                session_id: "c".into(),
                hint: hint("g1"),
            }],
            ..WindowState::empty("w1")
        }
    }

    fn json(layout: &Option<LayoutNode>) -> String {
        serde_json::to_string(layout).unwrap()
    }

    #[test]
    fn saves_a_window_with_its_sessions_collapsed_ones_included() {
        let sessions = records(&["a", "b", "c", "other"]);
        let ws = snapshot_window(&saved(), &sessions, "ws", "Frontend", 5.0);
        assert_eq!(ws.window.layout, Some(grouped()));
        assert_eq!(ws.window.bounds, Some(BOUNDS));
        let mut ids: Vec<&str> = ws.sessions.iter().map(|session| session.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn opens_as_a_window_of_its_own_linked_to_it_with_its_sessions_as_reframe_has_them_now() {
        let ws = snapshot_window(&saved(), &records(&["a", "b", "c"]), "ws", "F", 0.0);
        let session_ids = HashMap::from([("b".to_owned(), "b2".to_owned())]);
        let win = window_from_workspace(&ws, "w2", &session_ids, &HashSet::new(), || "new".to_owned());
        assert_eq!(win.id, "w2");
        assert_eq!(win.workspace_id.as_deref(), Some("ws"));
        assert!(!win.hidden);
        assert!(json(&win.layout).contains(r#""sessionId":"b2""#));
        assert!(json(&win.layout).contains(r#""id":"g1""#));
    }

    #[test]
    fn gives_a_group_a_new_id_when_that_one_is_in_use_elsewhere_and_follows_it_in_restore_hints() {
        let ws = snapshot_window(&saved(), &records(&["a", "b", "c"]), "ws", "F", 0.0);
        let session_ids = HashMap::from([("c".to_owned(), "c2".to_owned())]);
        let in_use = HashSet::from(["g1".to_owned()]);
        let win = window_from_workspace(&ws, "w2", &session_ids, &in_use, || "g9".to_owned());
        assert!(json(&win.layout).contains(r#""id":"g9""#));
        assert_eq!(
            win.dock[0],
            DockEntry::Tile {
                session_id: "c2".into(),
                hint: hint("g9"),
            }
        );
    }
}
