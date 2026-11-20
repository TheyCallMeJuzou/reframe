//! Moving the keyboard focus between tiles with arrow keys, by where the tiles are on screen.
//!
//! Ported from the Electron version's `src/renderer/focus-nav.ts`.

/// A tile on screen. `Box` in the Electron version; renamed to keep clear of `std::boxed::Box`.
#[derive(Clone, Debug, PartialEq)]
pub struct TileBox {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The tile next to `from_id` in `direction`: of the tiles on that side, the nearest one; among equally near
/// ones (a column of tiles beside a tall one), the one level with the current tile's center.
pub fn neighbor(boxes: &[TileBox], from_id: &str, direction: Direction) -> Option<String> {
    let from = boxes.iter().find(|b| b.id == from_id)?;
    let horizontal = matches!(direction, Direction::Left | Direction::Right);
    let center = if horizontal {
        from.y + from.height / 2.0
    } else {
        from.x + from.width / 2.0
    };
    // The best one so far, with its gap and offset.
    let mut best: Option<(&TileBox, f64, f64)> = None;
    for tile in boxes {
        if tile.id == from_id {
            continue;
        }
        let gap = match direction {
            Direction::Left => from.x - (tile.x + tile.width),
            Direction::Right => tile.x - (from.x + from.width),
            Direction::Up => from.y - (tile.y + tile.height),
            Direction::Down => tile.y - (from.y + from.height),
        };
        if gap < -1.0 {
            continue; // not on that side (a pixel of overlap is rounding)
        }
        let offset = if horizontal {
            outside(center, tile.y, tile.y + tile.height)
        } else {
            outside(center, tile.x, tile.x + tile.width)
        };
        let nearer = best.is_none_or(|(_, best_gap, best_offset)| {
            gap < best_gap - 1.0 || ((gap - best_gap).abs() <= 1.0 && offset < best_offset)
        });
        if nearer {
            best = Some((tile, gap, offset));
        }
    }
    best.map(|(tile, _, _)| tile.id.clone())
}

/// How far `value` lies outside the range from `min` to `max` (0 inside).
fn outside(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min - value
    } else if value > max {
        value - max
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "focus_nav_tests.rs"]
mod tests;
