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
/// The space around the layout.
pub const LAYOUT_PADDING: f32 = 6.0;

pub const UI_FONT: &str = "Segoe UI";
/// Windows' own icon font (on Windows 10 and 11).
pub const ICON_FONT: &str = "Segoe MDL2 Assets";

/// Glyphs of the icon font.
pub mod icon {
    pub const COLLAPSE: &str = "\u{E921}";
    pub const ZOOM: &str = "\u{E922}";
    pub const UNZOOM: &str = "\u{E923}";
    pub const CLOSE: &str = "\u{E8BB}";
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
    hex.and_then(|hex| u32::from_str_radix(hex, 16).ok()).map_or_else(|| rgb(ACCENT).into(), |value| rgb(value).into())
}
