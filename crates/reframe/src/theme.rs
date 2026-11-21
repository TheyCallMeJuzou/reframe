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

/// The fonts reframe draws with, chosen once at the start (see [`choose_fonts`]).
struct Fonts {
    ui: String,
    terminal: String,
    /// What the terminal falls back to for symbols its font lacks.
    symbols: Vec<String>,
}

static FONTS: std::sync::OnceLock<Fonts> = std::sync::OnceLock::new();

/// Windows' fonts, which 0.x drew with there.
fn windows_fonts() -> Fonts {
    Fonts {
        ui: "Segoe UI".into(),
        terminal: "Cascadia Mono".into(),
        symbols: vec!["Segoe UI Symbol".into()],
    }
}

/// What fontconfig gives for a generic family ("sans-serif", "monospace"), as Chromium drew 0.x's
/// `system-ui` and `monospace` on Linux.
#[cfg(not(windows))]
fn fontconfig_match(generic: &str) -> Option<String> {
    let output = std::process::Command::new("fc-match")
        .args(["--format=%{family[0]}", generic])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let family = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!family.is_empty()).then_some(family)
}

/// Picks the fonts: on Windows the ones 0.x drew with; on Linux the first installed of 0.x's
/// terminal fonts (Cascadia Mono, Cascadia Code, …, then the system's monospace font), and the
/// system's sans-serif font for everything else (0.x's `system-ui`). GPUI takes a family only by
/// its exact name, so only installed ones are chosen.
pub fn choose_fonts(cx: &App) {
    #[cfg(windows)]
    let fonts = {
        let _ = cx;
        windows_fonts()
    };
    #[cfg(not(windows))]
    let fonts = {
        let installed = cx.text_system().all_font_names();
        let have = |name: &str| installed.iter().any(|font| font == name);
        let first = |candidates: Vec<Option<String>>, last: &str| {
            candidates
                .into_iter()
                .flatten()
                .find(|name| have(name))
                .unwrap_or_else(|| last.to_owned())
        };
        let named = |names: &[&str]| names.iter().map(|name| Some((*name).to_owned())).collect::<Vec<_>>();
        let mut terminal = named(&[
            "Cascadia Mono",
            "Cascadia Code",
            "Consolas",
            "SF Mono",
            "Menlo",
            "DejaVu Sans Mono",
        ]);
        terminal.push(fontconfig_match("monospace"));
        terminal.extend(named(&["Noto Sans Mono", "Liberation Mono", "Ubuntu Mono"]));
        let mut ui = vec![fontconfig_match("sans-serif")];
        ui.extend(named(&[
            "Ubuntu Sans",
            "Ubuntu",
            "Cantarell",
            "Adwaita Sans",
            "Noto Sans",
            "DejaVu Sans",
        ]));
        Fonts {
            ui: first(ui, "DejaVu Sans"),
            terminal: first(terminal, "DejaVu Sans Mono"),
            symbols: ["Noto Sans Symbols 2", "Noto Sans Symbols", "DejaVu Sans"]
                .into_iter()
                .filter(|name| have(name))
                .map(str::to_owned)
                .collect(),
        }
    };
    let _ = FONTS.set(fonts);
}

fn fonts() -> &'static Fonts {
    FONTS.get_or_init(windows_fonts)
}

/// The font of everything but the terminals.
pub fn ui_font() -> &'static str {
    &fonts().ui
}

/// The terminals' font.
pub fn terminal_font() -> &'static str {
    &fonts().terminal
}

/// What the terminals fall back to for symbols their font lacks.
pub fn symbol_fonts() -> &'static [String] {
    &fonts().symbols
}
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
