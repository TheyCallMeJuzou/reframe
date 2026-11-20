use super::Direction::*;
use super::*;

fn tile(id: &str, x: f64, y: f64, width: f64, height: f64) -> TileBox {
    TileBox {
        id: id.to_owned(),
        x,
        y,
        width,
        height,
    }
}

/// a on the left, b / c / d stacked on the right (4 px splitters between them).
fn stacked() -> Vec<TileBox> {
    vec![
        tile("a", 0.0, 0.0, 500.0, 600.0),
        tile("b", 504.0, 0.0, 500.0, 196.0),
        tile("c", 504.0, 200.0, 500.0, 196.0),
        tile("d", 504.0, 404.0, 500.0, 196.0),
    ]
}

#[test]
fn moves_within_a_column_and_across_to_the_tall_tile() {
    let stacked = stacked();
    assert_eq!(neighbor(&stacked, "b", Down).as_deref(), Some("c"));
    assert_eq!(neighbor(&stacked, "d", Up).as_deref(), Some("c"));
    assert_eq!(neighbor(&stacked, "c", Left).as_deref(), Some("a"));
    assert_eq!(neighbor(&stacked, "d", Left).as_deref(), Some("a"));
}

#[test]
fn goes_from_the_tall_tile_to_the_one_level_with_its_center() {
    assert_eq!(neighbor(&stacked(), "a", Right).as_deref(), Some("c"));
}

#[test]
fn prefers_the_tile_straight_across_in_a_grid_over_a_diagonal_one() {
    let grid = [
        tile("a", 0.0, 0.0, 500.0, 300.0),
        tile("b", 0.0, 304.0, 500.0, 300.0),
        tile("c", 504.0, 0.0, 500.0, 300.0),
        tile("d", 504.0, 304.0, 500.0, 300.0),
    ];
    assert_eq!(neighbor(&grid, "a", Down).as_deref(), Some("b"));
    assert_eq!(neighbor(&grid, "a", Right).as_deref(), Some("c"));
    assert_eq!(neighbor(&grid, "d", Up).as_deref(), Some("c"));
}

#[test]
fn stays_put_at_the_edge_or_for_an_unknown_tile() {
    let stacked = stacked();
    assert_eq!(neighbor(&stacked, "a", Left), None);
    assert_eq!(neighbor(&stacked, "b", Up), None);
    assert_eq!(neighbor(&stacked, "zzz", Up), None);
}
