use super::*;

const GRID: GridAxis = GridAxis {
    start: 0.0,
    length: 1200.0,
    steps: 12,
};

/// A split over the whole layout without separator thickness: a grid line every 100 px (8.33 %).
const WHOLE: SplitAxis = SplitAxis {
    start: 0.0,
    length: 1200.0,
    separator: 0.0,
};

/// The sizes to two decimal places.
fn round(sizes: Vec<f64>) -> Vec<f64> {
    sizes.into_iter().map(|s| round_half_up(s * 100.0) / 100.0).collect()
}

#[test]
fn puts_the_separator_being_dragged_on_the_nearest_grid_line() {
    assert_eq!(
        round(snap_sizes(&[37.0, 63.0], &[50.0, 50.0], WHOLE, GRID)),
        [33.33, 66.67]
    );
    assert_eq!(
        round(snap_sizes(&[45.9, 54.1], &[50.0, 50.0], WHOLE, GRID)),
        [50.0, 50.0]
    );
}

#[test]
fn lines_up_with_the_grid_of_the_whole_layout_also_for_a_split_that_covers_only_part_of_it() {
    // The right half of the layout (600..1200): grid lines at 700, 800, … are at 1/6, 2/6, … of the split.
    let right_half = SplitAxis {
        start: 600.0,
        length: 600.0,
        separator: 0.0,
    };
    assert_eq!(
        round(snap_sizes(&[30.0, 70.0], &[50.0, 50.0], right_half, GRID)),
        [33.33, 66.67]
    );
}

#[test]
fn counts_the_separators_their_middle_lands_on_the_line() {
    let with_separator = SplitAxis {
        start: 0.0,
        length: 1206.0,
        separator: 6.0,
    };
    let first = snap_sizes(&[26.0, 74.0], &[50.0, 50.0], with_separator, GRID)[0];
    // The first panel ends 3 px before the line at 300, so the separator's middle is on it.
    assert_eq!(round_half_up((first / 100.0) * 1200.0), 297.0);
}

#[test]
fn leaves_separators_that_did_not_move_and_one_that_would_pass_its_neighbor() {
    // Three panels; only the second separator moved (from 66.67 to 80.5 %).
    assert_eq!(
        round(snap_sizes(&[33.33, 47.17, 19.5], &[33.33, 33.33, 33.34], WHOLE, GRID)),
        [33.33, 50.0, 16.67]
    );
    // The nearest line (8.33 %) is behind the first separator (at 10 %): it stays.
    assert_eq!(
        round(snap_sizes(&[10.0, 0.5, 89.5], &[10.0, 20.0, 70.0], WHOLE, GRID)),
        [10.0, 0.5, 89.5]
    );
}
