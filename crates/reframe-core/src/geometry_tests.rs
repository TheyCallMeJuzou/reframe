use super::*;
use crate::layout::tile;
use crate::state::GroupNode;

fn split(id: &str, dir: SplitDir, children: Vec<LayoutNode>, sizes: Vec<f64>) -> LayoutNode {
    LayoutNode::Split(SplitNode {
        id: id.into(),
        dir,
        children,
        sizes,
    })
}

fn group(id: &str, child: LayoutNode) -> LayoutNode {
    LayoutNode::Group(GroupNode {
        id: id.into(),
        name: "Group 1".into(),
        color: "#d97757".into(),
        child: Box::new(child),
    })
}

const AREA: Rect = Rect {
    x: 10.0,
    y: 20.0,
    width: 1006.0,
    height: 600.0,
};

#[test]
fn nothing_is_placed_without_a_layout() {
    assert_eq!(place(None, AREA, &Metrics::default()), Placement::default());
}

#[test]
fn a_single_tile_fills_the_area() {
    let placed = place(Some(&tile("a")), AREA, &Metrics::default());
    assert_eq!(
        placed.tiles,
        vec![PlacedTile {
            session_id: "a".into(),
            rect: AREA,
            group: None
        }]
    );
    assert!(placed.separators.is_empty() && placed.groups.is_empty());
}

#[test]
fn a_row_shares_its_width_and_leaves_room_for_separators() {
    let layout = split(
        "s",
        SplitDir::Row,
        vec![tile("a"), tile("b"), tile("c")],
        vec![50.0, 25.0, 25.0],
    );
    let placed = place(Some(&layout), AREA, &Metrics::default());
    // 1006 wide, two separators of 6: the tiles share 994.
    let rects: Vec<Rect> = placed.tiles.iter().map(|tile| tile.rect).collect();
    assert_eq!(rects[0], Rect::new(10.0, 20.0, 497.0, 600.0));
    assert_eq!(rects[1], Rect::new(513.0, 20.0, 248.5, 600.0));
    assert_eq!(rects[2], Rect::new(767.5, 20.0, 248.5, 600.0));
    assert_eq!(placed.separators.len(), 2);
    assert_eq!(placed.separators[0].rect, Rect::new(507.0, 20.0, 6.0, 600.0));
    assert_eq!(placed.separators[1].index, 1);
    assert_eq!(placed.separators[1].split, AREA);
}

#[test]
fn a_column_in_a_row_is_placed_inside_its_panel() {
    let column = split("c", SplitDir::Col, vec![tile("b"), tile("c")], vec![50.0, 50.0]);
    let layout = split("r", SplitDir::Row, vec![tile("a"), column], vec![50.0, 50.0]);
    let placed = place(Some(&layout), AREA, &Metrics::default());
    assert_eq!(placed.tile("b").unwrap().rect, Rect::new(516.0, 20.0, 500.0, 297.0));
    assert_eq!(placed.tile("c").unwrap().rect, Rect::new(516.0, 323.0, 500.0, 297.0));
    let inner = placed.separator("c", 0).unwrap();
    assert_eq!(inner.dir, SplitDir::Col);
    assert_eq!(inner.rect, Rect::new(516.0, 317.0, 500.0, 6.0));
}

#[test]
fn a_group_frames_its_tiles() {
    let layout = group(
        "g",
        split("s", SplitDir::Row, vec![tile("a"), tile("b")], vec![50.0, 50.0]),
    );
    let placed = place(Some(&layout), AREA, &Metrics::default());
    assert_eq!(placed.groups.len(), 1);
    assert_eq!(placed.groups[0].rect, AREA);
    assert_eq!(placed.groups[0].header, Rect::new(12.0, 22.0, 1002.0, 28.0));
    // Border 2 and padding 4 at the sides and below, border 2 and the bar of 28 above.
    let a = placed.tile("a").unwrap();
    assert_eq!(a.group.as_deref(), Some("g"));
    assert_eq!(a.rect, Rect::new(16.0, 50.0, 494.0, 564.0));
    assert_eq!(placed.tile("b").unwrap().rect, Rect::new(516.0, 50.0, 494.0, 564.0));
}

#[test]
fn sizes_that_do_not_fit_the_children_are_shared_evenly() {
    let layout = split("s", SplitDir::Row, vec![tile("a"), tile("b")], vec![100.0]);
    let placed = place(Some(&layout), AREA, &Metrics::default());
    assert_eq!(placed.tiles[0].rect.width, 500.0);
    assert_eq!(placed.tiles[1].rect.width, 500.0);
}

#[test]
fn an_area_too_small_gives_empty_rectangles_not_negative_ones() {
    let layout = group("g", tile("a"));
    let placed = place(Some(&layout), Rect::new(0.0, 0.0, 8.0, 8.0), &Metrics::default());
    assert_eq!((placed.tiles[0].rect.width, placed.tiles[0].rect.height), (0.0, 0.0));
}

#[test]
fn dragging_moves_space_between_the_two_panels_next_to_the_separator() {
    let sizes = drag_sizes(&[50.0, 25.0, 25.0], 0, 100.0, 1000.0, 100.0);
    assert_eq!(sizes, vec![60.0, 15.0, 25.0]);
    let sizes = drag_sizes(&[50.0, 25.0, 25.0], 1, -50.0, 1000.0, 160.0);
    assert_eq!(sizes, vec![50.0, 20.0, 30.0]);
}

#[test]
fn dragging_stops_at_the_smallest_size() {
    // The second panel has 500 pixels and keeps 160.
    assert_eq!(drag_sizes(&[50.0, 50.0], 0, 900.0, 1000.0, 160.0), vec![84.0, 16.0]);
    assert_eq!(drag_sizes(&[50.0, 50.0], 0, -900.0, 1000.0, 160.0), vec![16.0, 84.0]);
}

#[test]
fn a_panel_smaller_than_the_smallest_size_does_not_grow_by_itself() {
    // 100 pixels each: neither can give anything, so nothing moves.
    assert_eq!(drag_sizes(&[50.0, 50.0], 0, 30.0, 200.0, 160.0), vec![50.0, 50.0]);
}

#[test]
fn dragging_a_separator_that_is_not_there_changes_nothing() {
    assert_eq!(drag_sizes(&[50.0, 50.0], 1, 30.0, 1000.0, 160.0), vec![50.0, 50.0]);
    assert_eq!(drag_sizes(&[100.0], 0, 30.0, 1000.0, 160.0), vec![100.0]);
}

#[test]
fn rectangles_know_what_is_inside_them() {
    let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
    assert!(rect.contains(10.0, 10.0) && rect.contains(29.9, 29.9));
    assert!(!rect.contains(30.0, 15.0) && !rect.contains(9.9, 15.0));
}

/// Windows 11 at 100 %: 8 invisible pixels on the left, right and bottom, a 31 pixel title bar.
const FRAME_100: WindowFrame = WindowFrame {
    width: 16,
    height: 39,
    scale: 1.0,
};

#[test]
fn a_kept_window_is_the_whole_window_and_gpui_gets_its_client_area() {
    let outer = Bounds {
        x: 60.0,
        y: 60.0,
        width: 1280.0,
        height: 800.0,
    };
    let client = FRAME_100.client_of(outer);
    assert_eq!(
        client,
        Bounds {
            x: 68.0,
            y: 79.0,
            width: 1264.0,
            height: 761.0
        }
    );
    assert_eq!(FRAME_100.outer_of(client), outer);
}

#[test]
fn the_frame_is_in_device_pixels_and_the_bounds_are_not() {
    // 150 %: a frame of 23 × 56 device pixels.
    let frame = WindowFrame {
        width: 23,
        height: 56,
        scale: 1.5,
    };
    let outer = Bounds {
        x: 100.0,
        y: 40.0,
        width: 900.0,
        height: 600.0,
    };
    let client = frame.client_of(outer);
    assert!((client.x - (100.0 + 11.0 / 1.5)).abs() < 1e-9);
    assert!((client.y - (40.0 + 28.0 / 1.5)).abs() < 1e-9);
    assert!((client.width - (900.0 - 23.0 / 1.5)).abs() < 1e-9);
    assert!((client.height - (600.0 - 56.0 / 1.5)).abs() < 1e-9);
    let back = frame.outer_of(client);
    for (got, want) in [
        (back.x, outer.x),
        (back.y, outer.y),
        (back.width, outer.width),
        (back.height, outer.height),
    ] {
        assert!((got - want).abs() < 1e-9, "{got} != {want}");
    }
}
