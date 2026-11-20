//! Where a window's layout puts its tiles, group frames and separators, in pixels.
//!
//! The Electron version left this to the browser (flex boxes and a panel library); the native
//! window places everything itself, so the same numbers serve drawing, dragging separators,
//! snapping them to the grid and moving the focus between tiles.

use crate::state::{Bounds, LayoutNode, SplitDir, SplitNode};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// The rectangle without a margin on each side.
    pub fn inset(&self, left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect::new(
            self.x + left,
            self.y + top,
            self.width - left - right,
            self.height - top - bottom,
        )
    }
}

/// The sizes the layout is built from (as in the Electron version's style sheet).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The gap between two panels of a split, which is its separator.
    pub separator: f64,
    /// A group frame: its border, the bar above its tiles, and the space left, right and below.
    pub group_border: f64,
    pub group_header: f64,
    pub group_padding: f64,
    /// The least a panel keeps when a separator is dragged: its width in a row, its height in a
    /// column.
    pub min_width: f64,
    pub min_height: f64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            separator: 6.0,
            group_border: 2.0,
            group_header: 28.0,
            group_padding: 4.0,
            min_width: 160.0,
            min_height: 90.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedTile {
    pub session_id: String,
    pub rect: Rect,
    /// The group it sits in, if any (groups do not nest).
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedGroup {
    pub id: String,
    /// The whole frame.
    pub rect: Rect,
    /// The bar with its name.
    pub header: Rect,
}

/// The separator after a split's panel `index`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedSeparator {
    pub split_id: String,
    pub index: usize,
    pub dir: SplitDir,
    pub rect: Rect,
    /// The split it belongs to.
    pub split: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    pub tiles: Vec<PlacedTile>,
    pub groups: Vec<PlacedGroup>,
    pub separators: Vec<PlacedSeparator>,
}

impl Placement {
    pub fn tile(&self, session_id: &str) -> Option<&PlacedTile> {
        self.tiles.iter().find(|tile| tile.session_id == session_id)
    }

    pub fn separator(&self, split_id: &str, index: usize) -> Option<&PlacedSeparator> {
        self.separators
            .iter()
            .find(|separator| separator.split_id == split_id && separator.index == index)
    }
}

/// A split's sizes as shares that sum to 1, one per child. Sizes that do not fit the children
/// (there should be none) give every child the same share.
pub fn shares(split: &SplitNode) -> Vec<f64> {
    let count = split.children.len();
    let total: f64 = split.sizes.iter().sum();
    let usable = split.sizes.len() == count && total > 0.0 && split.sizes.iter().all(|size| *size >= 0.0);
    if usable {
        split.sizes.iter().map(|size| size / total).collect()
    } else {
        vec![1.0 / count.max(1) as f64; count]
    }
}

/// How long a split is along its direction, and how much of that its panels share (the rest
/// are separators).
pub fn split_space(split: &SplitNode, rect: Rect, metrics: &Metrics) -> (f64, f64) {
    let length = if split.dir == SplitDir::Row {
        rect.width
    } else {
        rect.height
    };
    let separators = split.children.len().saturating_sub(1) as f64 * metrics.separator;
    (length, (length - separators).max(0.0))
}

/// Places a layout in an area.
pub fn place(layout: Option<&LayoutNode>, area: Rect, metrics: &Metrics) -> Placement {
    let mut placement = Placement::default();
    if let Some(node) = layout {
        place_node(node, area, None, metrics, &mut placement);
    }
    placement
}

fn place_node(node: &LayoutNode, rect: Rect, group: Option<&str>, metrics: &Metrics, out: &mut Placement) {
    match node {
        LayoutNode::Tile(tile) => out.tiles.push(PlacedTile {
            session_id: tile.session_id.clone(),
            rect,
            group: group.map(str::to_owned),
        }),
        LayoutNode::Group(frame) => {
            let (border, padding) = (metrics.group_border, metrics.group_padding);
            let header = Rect::new(
                rect.x + border,
                rect.y + border,
                rect.width - 2.0 * border,
                metrics.group_header,
            );
            out.groups.push(PlacedGroup {
                id: frame.id.clone(),
                rect,
                header,
            });
            let side = border + padding;
            let inner = rect.inset(side, border + metrics.group_header, side, side);
            place_node(&frame.child, inner, Some(&frame.id), metrics, out);
        }
        LayoutNode::Split(split) => {
            let row = split.dir == SplitDir::Row;
            let (_, available) = split_space(split, rect, metrics);
            let mut offset = 0.0;
            let count = split.children.len();
            for (index, (child, share)) in split.children.iter().zip(shares(split)).enumerate() {
                let length = share * available;
                let child_rect = if row {
                    Rect::new(rect.x + offset, rect.y, length, rect.height)
                } else {
                    Rect::new(rect.x, rect.y + offset, rect.width, length)
                };
                place_node(child, child_rect, group, metrics, out);
                offset += length;
                if index + 1 < count {
                    let separator = if row {
                        Rect::new(rect.x + offset, rect.y, metrics.separator, rect.height)
                    } else {
                        Rect::new(rect.x, rect.y + offset, rect.width, metrics.separator)
                    };
                    out.separators.push(PlacedSeparator {
                        split_id: split.id.clone(),
                        index,
                        dir: split.dir,
                        rect: separator,
                        split: rect,
                    });
                    offset += metrics.separator;
                }
            }
        }
    }
}

/// A split's sizes (percentages) after the separator behind panel `index` was dragged by `delta`
/// pixels: the two panels next to it share what they had, and neither gets smaller than `min`
/// pixels (or than it was, if it was smaller already). `available` is what the panels share in
/// all, in pixels.
pub fn drag_sizes(before: &[f64], index: usize, delta: f64, available: f64, min: f64) -> Vec<f64> {
    if index + 1 >= before.len() || available <= 0.0 {
        return before.to_vec();
    }
    let total: f64 = before.iter().sum();
    if total <= 0.0 {
        return before.to_vec();
    }
    let pixels = |size: f64| size / total * available;
    let (first, second) = (pixels(before[index]), pixels(before[index + 1]));
    // How far it can go either way.
    let back = (first - min.min(first)).max(0.0);
    let forth = (second - min.min(second)).max(0.0);
    let moved = delta.clamp(-back, forth);
    let mut sizes = before.to_vec();
    sizes[index] = (first + moved) / available * total;
    sizes[index + 1] = (second - moved) / available * total;
    sizes
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;

/// A whole window, frame and all, as the state keeps it (Electron's bounds): 1280 × 800 when
/// nothing is kept, in the middle of the screen.
pub const DEFAULT_WINDOW_SIZE: (f64, f64) = (1280.0, 800.0);

/// The frame Windows draws around a window's client area (title bar, borders and the invisible
/// borders to resize by): how much wider and taller the whole window is, in device pixels, and
/// how many of those make one of the state's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowFrame {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

impl WindowFrame {
    /// Where the client area of a window with these `outer` bounds is, the way GPUI places a
    /// client area in its window: half of the frame's width and height (rounded down) before it,
    /// the rest after it.
    pub fn client_of(&self, outer: Bounds) -> Bounds {
        Bounds {
            x: outer.x + f64::from(self.width / 2) / self.scale,
            y: outer.y + f64::from(self.height / 2) / self.scale,
            width: outer.width - f64::from(self.width) / self.scale,
            height: outer.height - f64::from(self.height) / self.scale,
        }
    }

    /// The whole window around a client area GPUI reports (the other way round).
    pub fn outer_of(&self, client: Bounds) -> Bounds {
        Bounds {
            x: client.x - f64::from(self.width / 2) / self.scale,
            y: client.y - f64::from(self.height / 2) / self.scale,
            width: client.width + f64::from(self.width) / self.scale,
            height: client.height + f64::from(self.height) / self.scale,
        }
    }
}
