//! reframe's small icons, the outlines of the Electron version's (`src/renderer/components/icons.tsx`):
//! 16 units wide, drawn 14 pixels wide in the colour the element is given.

use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Collapse,
    Close,
    Window,
    Info,
    Gear,
    Layers,
    ExitFullscreen,
    Plus,
    List,
    More,
    ChevronUp,
    ChevronDown,
    Zoom,
    Unzoom,
    Grip,
}

/// Outlines: drawn with a 1.6 unit line.
macro_rules! stroked {
    ($body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="white" stroke-width="1.6">"#,
            $body,
            "</svg>"
        )
    };
}

/// Filled shapes.
macro_rules! filled {
    ($body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16" fill="white">"#,
            $body,
            "</svg>"
        )
    };
}

impl Icon {
    fn svg(self) -> &'static str {
        match self {
            Icon::Collapse => {
                stroked!(r#"<path d="M10 3.5 5.5 8l4.5 4.5" stroke-linecap="round" stroke-linejoin="round"/>"#)
            }
            Icon::Close => stroked!(r#"<path d="m4 4 8 8M12 4l-8 8" stroke-linecap="round"/>"#),
            Icon::Window => {
                stroked!(r#"<rect x="2.5" y="3" width="11" height="10" rx="1.5"/><path d="M2.5 6h11"/>"#)
            }
            Icon::Info => {
                stroked!(r#"<circle cx="8" cy="8" r="6"/><path d="M8 7.2v4M8 4.8v.01" stroke-linecap="round"/>"#)
            }
            Icon::Gear => stroked!(
                r#"<circle cx="8" cy="8" r="2.2"/><path d="M8 1.6v1.8M8 12.6v1.8M1.6 8h1.8M12.6 8h1.8M3.5 3.5l1.3 1.3M11.2 11.2l1.3 1.3M3.5 12.5l1.3-1.3M11.2 4.8l1.3-1.3" stroke-linecap="round"/><circle cx="8" cy="8" r="4.4"/>"#
            ),
            Icon::Layers => stroked!(
                r#"<path d="M8 2.5 14 5.5 8 8.5 2 5.5z" stroke-linejoin="round"/><path d="m2 8.2 6 3 6-3M2 10.9l6 3 6-3" stroke-linejoin="round"/>"#
            ),
            Icon::ExitFullscreen => stroked!(
                r#"<path d="M6 2.5V6H2.5M10 2.5V6h3.5M6 13.5V10H2.5M10 13.5V10h3.5" stroke-linecap="round" stroke-linejoin="round"/>"#
            ),
            Icon::Plus => stroked!(r#"<path d="M8 3v10M3 8h10" stroke-linecap="round"/>"#),
            Icon::List => stroked!(r#"<path d="M3 4h10M3 8h10M3 12h10" stroke-linecap="round"/>"#),
            Icon::More => filled!(
                r#"<circle cx="4" cy="8" r="1.3"/><circle cx="8" cy="8" r="1.3"/><circle cx="12" cy="8" r="1.3"/>"#
            ),
            Icon::ChevronUp => {
                stroked!(r#"<path d="M3.5 10 8 5.5l4.5 4.5" stroke-linecap="round" stroke-linejoin="round"/>"#)
            }
            Icon::ChevronDown => {
                stroked!(r#"<path d="M3.5 6 8 10.5 12.5 6" stroke-linecap="round" stroke-linejoin="round"/>"#)
            }
            Icon::Zoom => stroked!(
                r#"<path d="M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5 9 7M2.5 13.5 7 9" stroke-linecap="round" stroke-linejoin="round"/>"#
            ),
            Icon::Unzoom => stroked!(
                r#"<path d="M13 7H9V3M3 9h4v4M9 7l4.5-4.5M7 9l-4.5 4.5" stroke-linecap="round" stroke-linejoin="round"/>"#
            ),
            Icon::Grip => filled!(
                r#"<circle cx="6" cy="5" r="1.1"/><circle cx="10" cy="5" r="1.1"/><circle cx="6" cy="8" r="1.1"/><circle cx="10" cy="8" r="1.1"/><circle cx="6" cy="11" r="1.1"/><circle cx="10" cy="11" r="1.1"/>"#
            ),
        }
    }
}

/// The icon, 14 pixels wide; give it a colour with `text_color` (and `group_hover` for one that
/// follows its button).
pub fn icon(icon: Icon) -> Svg {
    svg().data(icon.svg().as_bytes()).size(px(14.0)).flex_none()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's `test` attribute would come along.
    use super::Icon;

    const ALL: [Icon; 15] = [
        Icon::Collapse,
        Icon::Close,
        Icon::Window,
        Icon::Info,
        Icon::Gear,
        Icon::Layers,
        Icon::ExitFullscreen,
        Icon::Plus,
        Icon::List,
        Icon::More,
        Icon::ChevronUp,
        Icon::ChevronDown,
        Icon::Zoom,
        Icon::Unzoom,
        Icon::Grip,
    ];

    /// Each icon is an SVG the renderer can read, with something to draw in it.
    #[test]
    fn every_icon_is_a_drawable_svg() {
        for icon in ALL {
            let tree = usvg::Tree::from_str(icon.svg(), &usvg::Options::default())
                .unwrap_or_else(|error| panic!("{icon:?}: {error}"));
            assert!(tree.root().has_children(), "{icon:?} draws nothing");
            assert_eq!(tree.size().width(), 16.0, "{icon:?}");
        }
    }
}
