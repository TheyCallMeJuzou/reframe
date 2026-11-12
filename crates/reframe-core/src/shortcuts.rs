//! reframe's keyboard shortcuts. They use Ctrl+Shift (Cmd+Shift on macOS), plus Ctrl+1…9, Ctrl+, (settings), F11
//! and the text size keys (Ctrl with +, - and 0, as in Windows Terminal), because Claude Code itself takes many
//! plain Ctrl keys; reframe catches these before the terminal sees them.
//!
//! Ported from the Electron version's `src/renderer/shortcuts.ts`.

use crate::focus_nav::Direction;
use crate::state::{FONT_SIZE_MAX, FONT_SIZE_MIN, FONT_SIZE_NORMAL};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextSizeChange {
    Bigger,
    Smaller,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shortcut {
    /// The tile at this place (from 0) gets the focus.
    FocusTile {
        index: usize,
    },
    FocusDirection {
        direction: Direction,
    },
    NewSession,
    NewWindow,
    NextWaiting,
    Find,
    Zoom,
    ToggleCollapse,
    ToggleSidebar,
    ToggleFullscreen,
    OpenSettings,
    TextSize {
        change: TextSizeChange,
    },
}

/// A key press: `code` is the physical key and `key` the character it makes, both named as in the browser's
/// `KeyboardEvent` ("KeyT" and "t", "Digit3" and "3", "Comma" and ","). The Electron version's `ctrlKey`,
/// `metaKey`, `shiftKey` and `altKey` are `ctrl`, `meta`, `shift` and `alt` here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys<'a> {
    pub code: &'a str,
    pub key: &'a str,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
    pub alt: bool,
}

fn arrow(code: &str) -> Option<Direction> {
    match code {
        "ArrowLeft" => Some(Direction::Left),
        "ArrowRight" => Some(Direction::Right),
        "ArrowUp" => Some(Direction::Up),
        "ArrowDown" => Some(Direction::Down),
        _ => None,
    }
}

fn letter(code: &str) -> Option<Shortcut> {
    match code {
        "KeyT" => Some(Shortcut::NewSession),
        "KeyN" => Some(Shortcut::NewWindow),
        "KeyM" => Some(Shortcut::ToggleCollapse),
        "KeyL" => Some(Shortcut::ToggleSidebar),
        "Space" => Some(Shortcut::NextWaiting),
        "KeyF" => Some(Shortcut::Find),
        "KeyZ" => Some(Shortcut::Zoom),
        _ => None,
    }
}

/// By character, not key: they sit elsewhere on other layouts (+ has a key of its own on a German keyboard).
fn text_size(key: &str) -> Option<TextSizeChange> {
    match key {
        "+" | "=" => Some(TextSizeChange::Bigger),
        "-" => Some(TextSizeChange::Smaller),
        "0" => Some(TextSizeChange::Reset),
        _ => None,
    }
}

/// The digit of "Digit1" to "Digit9" (the Electron version's `/^Digit([1-9])$/`).
fn digit(code: &str) -> Option<usize> {
    match code.strip_prefix("Digit")?.as_bytes() {
        [digit @ b'1'..=b'9'] => Some(usize::from(digit - b'0')),
        _ => None,
    }
}

/// The shortcut a key press means, if any (by physical key, so it does not depend on the keyboard layout).
pub fn shortcut_for(keys: Keys<'_>, mac: bool) -> Option<Shortcut> {
    if keys.code == "F11" && !keys.ctrl && !keys.meta && !keys.shift && !keys.alt {
        return Some(Shortcut::ToggleFullscreen);
    }
    let modifier = if mac {
        keys.meta && !keys.ctrl
    } else {
        keys.ctrl && !keys.meta
    };
    if !modifier || keys.alt {
        return None;
    }
    // Shift only for + (Shift and = on US keyboards); Ctrl+Shift+- is Ctrl+_, Claude Code's undo.
    if let Some(change) = text_size(keys.key)
        && (!keys.shift || keys.key == "+")
    {
        return Some(Shortcut::TextSize { change });
    }
    if keys.code == "Comma" && !keys.shift {
        return Some(Shortcut::OpenSettings);
    }
    if let Some(digit) = digit(keys.code)
        && !keys.shift
    {
        return Some(Shortcut::FocusTile { index: digit - 1 });
    }
    if !keys.shift {
        return None;
    }
    if let Some(direction) = arrow(keys.code) {
        return Some(Shortcut::FocusDirection { direction });
    }
    letter(keys.code)
}

/// The font size a text size change leads to, within the range.
pub fn font_size_after(size: u8, change: TextSizeChange) -> u8 {
    // Saturating, so that a size at the end of what a `u8` holds stays a size (a JavaScript number has no such end).
    match change {
        TextSizeChange::Reset => FONT_SIZE_NORMAL,
        TextSizeChange::Bigger => size.saturating_add(1).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX),
        TextSizeChange::Smaller => size.saturating_sub(1).clamp(FONT_SIZE_MIN, FONT_SIZE_MAX),
    }
}

/// The terminals' font size as a window keeps it: changed at once, with the saved size coming back later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSizeState {
    pub size: u8,
    /// A change sent to be saved whose echo is still to come; the echoes of earlier changes are passed over.
    pub awaited: Option<u8>,
}

/// The state after a change, or `None` when the size stays (at the end of its range already).
pub fn change_font_size(state: FontSizeState, change: TextSizeChange) -> Option<FontSizeState> {
    let size = font_size_after(state.size, change);
    (size != state.size).then_some(FontSizeState {
        size,
        awaited: Some(size),
    })
}

/// The state once a saved size comes back: taken, unless a later change of this window is still on its way.
pub fn echo_font_size(state: FontSizeState, saved: u8) -> FontSizeState {
    if state.awaited.is_some_and(|awaited| saved != awaited) {
        return state;
    }
    FontSizeState {
        size: saved,
        awaited: None,
    }
}

/// What Ctrl and Shift are called in the user's language, for `shortcut_label`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModifierWords<'a> {
    pub ctrl: &'a str,
    pub shift: &'a str,
}

/// How a shortcut is written in hints, e.g. "Ctrl+Shift+T" ("Strg+Umschalt+T"), or "⌘⇧T" on macOS.
///
/// `shift` says whether the shortcut has Shift in it. The Electron version leaves it out for `true`; Rust has
/// no default arguments, so it is always given here.
pub fn shortcut_label(key: &str, words: ModifierWords<'_>, mac: bool, shift: bool) -> String {
    if mac {
        return format!("⌘{}{key}", if shift { "⇧" } else { "" });
    }
    if shift {
        format!("{}+{}+{key}", words.ctrl, words.shift)
    } else {
        format!("{}+{key}", words.ctrl)
    }
}

#[cfg(test)]
#[path = "shortcuts_tests.rs"]
mod tests;
