//! Snapping separators to a grid over the whole layout, so that separators of different splits (side by side, or
//! in different groups) line up. Pure geometry: positions along one axis, in pixels.
//!
//! Ported from the Electron version's `src/renderer/snap.ts`.

/// Grid lines across the layout.
pub const GRID_STEPS: u32 = 12;

/// A split along its axis: where it starts, how long it is, and how thick each separator between its panels is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitAxis {
    pub start: f64,
    pub length: f64,
    pub separator: f64,
}

/// The grid along the same axis: the layout's start and length, divided into `steps`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridAxis {
    pub start: f64,
    pub length: f64,
    pub steps: u32,
}

/// The split's panel sizes (percentages) with every separator that moved since `before` on the nearest grid line,
/// unless that would push it past a neighbor. Separators that did not move stay where they are.
pub fn snap_sizes(sizes: &[f64], before: &[f64], split: SplitAxis, grid: GridAxis) -> Vec<f64> {
    if sizes.len() != before.len() || sizes.len() < 2 {
        return sizes.to_vec();
    }
    let available = split.length - (sizes.len() - 1) as f64 * split.separator;
    if available <= 0.0 || grid.length <= 0.0 {
        return sizes.to_vec();
    }
    let step = grid.length / f64::from(grid.steps);
    let now = prefix(sizes);
    let was = prefix(before);
    // Where separator i's middle is for a given share of the split before it, and back.
    let position = |share: f64, i: usize| {
        split.start + (share / 100.0) * available + i as f64 * split.separator + split.separator / 2.0
    };
    let share =
        |x: f64, i: usize| ((x - split.start - i as f64 * split.separator - split.separator / 2.0) / available) * 100.0;

    let mut snapped = now.clone();
    for i in 0..sizes.len() - 1 {
        if (now[i] - was[i]).abs() < 0.01 {
            continue;
        }
        let line = grid.start + round_half_up((position(now[i], i) - grid.start) / step) * step;
        let target = share(line, i);
        let low = if i > 0 { snapped[i - 1] } else { 0.0 };
        let high = now[i + 1];
        if target > low && target < high {
            snapped[i] = target;
        }
    }
    (0..snapped.len())
        .map(|i| snapped[i] - if i > 0 { snapped[i - 1] } else { 0.0 })
        .collect()
}

/// The running totals of `list`: where each panel ends.
fn prefix(list: &[f64]) -> Vec<f64> {
    list.iter()
        .scan(0.0, |total, size| {
            *total += size;
            Some(*total)
        })
        .collect()
}

/// JavaScript's `Math.round`: halves go up (-2.5 becomes -2), where `f64::round` takes them away from zero.
fn round_half_up(value: f64) -> f64 {
    (value + 0.5).floor()
}

#[cfg(test)]
#[path = "snap_tests.rs"]
mod tests;
