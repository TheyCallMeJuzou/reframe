//! reframe's colours and sizes (the Electron version's style sheet, `src/renderer/styles.css`).

use gpui_kit::*;
use reframe_core::session_logic::SessionStatus;

pub const BG: u32 = 0x1b1c20;
pub const SURFACE: u32 = 0x22242a;
pub const SURFACE_2: u32 = 0x2a2d34;
pub const TERMINAL: u32 = 0x17181c;
pub const BORDER: u32 = 0x30333b;
pub const BORDER_STRONG: u32 = 0x454a55;
pub const TEXT: u32 = 0xd7dae0;
pub const MUTED: u32 = 0x8a8f98;
pub const ACCENT: u32 = 0xd97757;
pub const ACCENT_TEXT: u32 = 0x1b1c20;
pub const OK: u32 = 0x4fbf7f;
pub const WARN: u32 = 0xe0b04a;
pub const DANGER: u32 = 0xe06060;

pub const RADIUS: f32 = 6.0;
pub const TILE_HEADER: f32 = 30.0;
pub const TOOLBAR: f32 = 40.0;
/// The space around the layout.
pub const LAYOUT_PADDING: f32 = 6.0;

pub const UI_FONT: &str = "Segoe UI";
/// The UI's line height, as a share of the font size (0.x's `font: 13px/1.4`; GPUI's own is 1.618).
pub const LINE_HEIGHT: f32 = 1.4;

/// A text field as 0.x drew one (`padding: 6px 8px`, 13 px text): 32 pixels high.
pub fn field<E: gpui_kit::Styled>(input: E) -> E {
    input
        .h(px(32.0))
        .px(px(8.0))
        .text_size(px(13.0))
        .bg(rgb(BG))
        .border_color(rgb(BORDER_STRONG))
}

/// The colour of a status dot.
pub fn status_color(status: SessionStatus) -> u32 {
    match status {
        SessionStatus::Starting => MUTED,
        SessionStatus::Idle => OK,
        SessionStatus::Busy => ACCENT,
        SessionStatus::Waiting => WARN,
        SessionStatus::Ended => BORDER_STRONG,
        SessionStatus::Error => DANGER,
    }
}

/// A colour written as `#rrggbb` (a group's), or the accent if it is not one.
pub fn parse_color(text: &str) -> Hsla {
    let hex = text.strip_prefix('#').filter(|hex| hex.len() == 6);
    hex.and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .map_or_else(|| rgb(ACCENT).into(), |value| rgb(value).into())
}
