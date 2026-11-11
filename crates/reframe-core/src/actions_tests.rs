// The original's two cases for `isDropTarget` and `isLayoutAction` have no counterpart: those
// validators are not ported (see actions.rs).
use super::*;
use crate::layout::{NewGroup, add_to_group, group_tile};
use crate::state::Side;

fn with_tiles(ids: &[&str]) -> WindowState {
    ids.iter()
        .fold(WindowState::empty("w1"), |win, id| add_tile(&win, id, None))
}

fn collapse_tile(session_id: &str) -> LayoutAction {
    LayoutAction::TileCollapse {
        session_id: session_id.into(),
    }
}

fn expand_tile(session_id: &str) -> LayoutAction {
    LayoutAction::TileExpand {
        session_id: session_id.into(),
    }
}

fn collapse_group(group_id: &str) -> LayoutAction {
    LayoutAction::GroupCollapse {
        group_id: group_id.into(),
    }
}

fn rename_group(group_id: &str, name: &str) -> LayoutAction {
    LayoutAction::GroupRename {
        group_id: group_id.into(),
        name: name.into(),
    }
}

fn new_group() -> NewGroup {
    NewGroup {
        id: Some("g".into()),
        name: "G".into(),
        color: "#f00".into(),
    }
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

fn docked_sessions(win: &WindowState) -> Vec<String> {
    win.dock.iter().flat_map(dock_sessions).collect()
}

fn group_ids(win: &WindowState) -> Vec<&str> {
    window_groups(win).iter().map(|group| group.id.as_str()).collect()
}

// ---------- collapse and expand ----------

#[test]
fn moves_a_tile_into_the_dock_and_back_to_its_place() {
    let win = with_tiles(&["a", "b", "c"]);
    let collapsed = apply_layout_action(&win, &collapse_tile("b"));
    assert_eq!(leaves(collapsed.layout.as_ref()), ["a", "c"]);
    assert_eq!(docked_sessions(&collapsed), ["b"]);
    assert!(window_has_session(&collapsed, "b"));

    let expanded = apply_layout_action(&collapsed, &expand_tile("b"));
    assert_eq!(leaves(expanded.layout.as_ref()), ["a", "b", "c"]);
    assert_eq!(expanded.dock, []);
}

#[test]
fn ignores_unknown_sessions() {
    let win = with_tiles(&["a"]);
    assert_eq!(apply_layout_action(&win, &collapse_tile("x")), win);
    assert_eq!(apply_layout_action(&win, &expand_tile("x")), win);
}

// ---------- dropSession ----------

#[test]
fn removes_a_session_from_layout_and_dock() {
    let mut win = with_tiles(&["a", "b"]);
    win = apply_layout_action(&win, &collapse_tile("b"));
    win = drop_session(&drop_session(&win, "b"), "a");
    assert_eq!(win.layout, None);
    assert_eq!(win.dock, []);
}

// ---------- isWindowEmpty ----------

#[test]
fn counts_collapsed_tiles_as_content() {
    assert!(is_window_empty(&WindowState::empty("w")));
    let collapsed = apply_layout_action(&with_tiles(&["a"]), &collapse_tile("a"));
    assert_eq!(collapsed.layout, None);
    assert!(!is_window_empty(&collapsed));
    assert!(is_window_empty(&drop_session(&collapsed, "a")));
}

// ---------- windowsToRestore ----------

#[test]
fn leaves_empty_windows_out_and_keeps_the_first_one_only_when_all_are_empty() {
    let tiles = add_tile(&WindowState::empty("tiles"), "a", None);
    let collapsed = apply_layout_action(
        &add_tile(&WindowState::empty("collapsed"), "b", None),
        &collapse_tile("b"),
    );
    let windows = [
        WindowState::empty("x"),
        tiles.clone(),
        WindowState::empty("y"),
        collapsed.clone(),
    ];
    assert_eq!(windows_to_restore(&windows), [tiles, collapsed]);
    assert_eq!(
        windows_to_restore(&[WindowState::empty("x"), WindowState::empty("y")]),
        [WindowState::empty("x")]
    );
}

// ---------- groups in a window ----------

fn grouped() -> WindowState {
    let mut win = with_tiles(&["a", "b", "c"]);
    win.layout = group_tile(win.layout.as_ref(), "a", new_group());
    win.layout = add_to_group(win.layout.as_ref(), "b", "g");
    win
}

#[test]
fn collapses_a_whole_group_into_one_dock_entry_and_restores_it() {
    let collapsed = apply_layout_action(&grouped(), &collapse_group("g"));
    assert_eq!(leaves(collapsed.layout.as_ref()), ["c"]);
    assert_eq!(collapsed.dock.len(), 1);
    assert_eq!(sorted(window_sessions(&collapsed)), ["a", "b", "c"]);
    assert!(is_collapsed(&collapsed, "a"));
    assert_eq!(group_ids(&collapsed), ["g"]);

    let expanded = apply_layout_action(&collapsed, &LayoutAction::GroupExpand { group_id: "g".into() });
    assert_eq!(leaves(expanded.layout.as_ref()), ["a", "b", "c"]);
    assert_eq!(
        group_of(expanded.layout.as_ref(), &tile_key("b")).map(|group| group.id.as_str()),
        Some("g")
    );
    assert_eq!(expanded.dock, []);
}

#[test]
fn collapsing_the_last_tile_of_a_group_takes_the_group_along() {
    let mut win = with_tiles(&["a", "b"]);
    win.layout = group_tile(win.layout.as_ref(), "a", new_group());
    let collapsed = apply_layout_action(&win, &collapse_tile("a"));
    assert!(matches!(collapsed.dock.as_slice(), [DockEntry::Group { node, .. }] if node.id == "g"));
}

#[test]
fn renames_a_group_visible_or_collapsed() {
    let renamed = apply_layout_action(&grouped(), &rename_group("g", "  API  "));
    assert_eq!(window_groups(&renamed)[0].name, "API");
    let collapsed = apply_layout_action(&renamed, &collapse_group("g"));
    let again = apply_layout_action(&collapsed, &rename_group("g", "Backend"));
    assert_eq!(window_groups(&again)[0].name, "Backend");
    assert_eq!(apply_layout_action(&again, &rename_group("g", "   ")), again);
    let long = apply_layout_action(&again, &rename_group("g", &"x".repeat(500)));
    assert_eq!(window_groups(&long)[0].name.encode_utf16().count(), 80);
}

#[test]
fn drops_a_session_out_of_a_collapsed_group_and_the_group_once_it_is_empty() {
    let collapsed = apply_layout_action(&grouped(), &collapse_group("g"));
    let one = drop_session(&collapsed, "a");
    assert_eq!(sorted(window_sessions(&one)), ["b", "c"]);
    assert_eq!(drop_session(&one, "b").dock, []);
}

#[test]
fn takes_a_collapsed_tile_out_of_the_side_bar_and_one_tile_out_of_a_collapsed_group() {
    let docked = apply_layout_action(&with_tiles(&["a", "b"]), &collapse_tile("b"));
    let Taken { win: rest, node } = take_node(&docked, &tile_key("b"));
    assert_eq!(node, Some(tile("b")));
    assert_eq!(rest.dock, []);

    let collapsed = apply_layout_action(&grouped(), &collapse_group("g"));
    let taken = take_node(&collapsed, &tile_key("a"));
    assert_eq!(taken.node, Some(tile("a")));
    assert!(matches!(
        taken.win.dock.as_slice(),
        [DockEntry::Group { node, .. }] if node.id == "g" && *node.child == tile("b")
    ));
    // The last one takes the group along.
    assert_eq!(take_node(&taken.win, &tile_key("b")).win.dock, []);
}

#[test]
fn moves_a_collapsed_tile_next_to_a_target_or_to_the_end_without_one() {
    let docked = apply_layout_action(&with_tiles(&["a", "b", "c"]), &collapse_tile("b"));
    let next = apply_layout_action(
        &docked,
        &LayoutAction::NodeMove {
            key: tile_key("b"),
            target: Some(DropTarget {
                key: tile_key("a"),
                side: Side::Bottom,
            }),
        },
    );
    assert_eq!(next.dock, []);
    assert_eq!(sorted(window_sessions(&next)), ["a", "b", "c"]);
    let move_to_end = |session_id: &str| LayoutAction::NodeMove {
        key: tile_key(session_id),
        target: None,
    };
    let at_end = apply_layout_action(&docked, &move_to_end("b"));
    assert_eq!(at_end.dock, []);
    assert!(leaves(at_end.layout.as_ref()).contains(&"b".to_owned()));
    // One that is shown already stays where it is without a target.
    assert_eq!(apply_layout_action(&docked, &move_to_end("a")), docked);
}

#[test]
fn moves_a_group_between_windows() {
    let from = grouped();
    let Taken { win: rest, node } = take_node(&from, "g");
    assert_eq!(leaves(rest.layout.as_ref()), ["c"]);
    let node = node.unwrap();
    let target = DropTarget {
        key: tile_key("x"),
        side: Side::Bottom,
    };
    let to = put_node(&with_tiles(&["x"]), node.clone(), Some(&target));
    assert_eq!(
        group_of(to.layout.as_ref(), &tile_key("a")).map(|group| group.id.as_str()),
        Some("g")
    );
    assert_eq!(
        put_node(&WindowState::empty("w2"), node.clone(), None).layout,
        Some(node)
    );
}
