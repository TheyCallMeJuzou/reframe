//! From a key press as the window gets it to reframe's shortcut for it.
//!
//! `reframe_core::shortcuts` speaks of keys as the browser does (the Electron version's
//! `KeyboardEvent`); this names GPUI's keys that way.

use gpui_kit::Keystroke;
use reframe_core::i18n::{Locale, t};
use reframe_core::shortcuts::{Keys, ModifierWords, Shortcut, shortcut_for, shortcut_label};

/// The shortcut a key press means, if any.
pub fn shortcut_of(keystroke: &Keystroke) -> Option<Shortcut> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();
    let code = match key {
        "left" => "ArrowLeft".to_owned(),
        "right" => "ArrowRight".to_owned(),
        "up" => "ArrowUp".to_owned(),
        "down" => "ArrowDown".to_owned(),
        "space" => "Space".to_owned(),
        "," => "Comma".to_owned(),
        "f11" => "F11".to_owned(),
        _ => match key.as_bytes() {
            [letter] if letter.is_ascii_alphabetic() => format!("Key{}", key.to_ascii_uppercase()),
            [digit] if digit.is_ascii_digit() => format!("Digit{key}"),
            _ => String::new(),
        },
    };
    // GPUI names a key by what it types without Shift; + is Shift and = on US keyboards.
    let character = if key == "=" && modifiers.shift { "+" } else { key };
    shortcut_for(
        Keys {
            code: &code,
            key: character,
            ctrl: modifiers.control,
            meta: modifiers.platform,
            shift: modifiers.shift,
            alt: modifiers.alt,
        },
        false,
    )
}

/// How a shortcut is written in hints: "Ctrl+Shift+T", or "Ctrl+," without Shift.
pub fn label(locale: Locale, key: &str, shift: bool) -> String {
    let word = |key: &str| t(locale, key, &[]).unwrap_or_default();
    let (ctrl, shift_word) = (word("keys.ctrl"), word("keys.shift"));
    shortcut_label(
        key,
        ModifierWords {
            ctrl: &ctrl,
            shift: &shift_word,
        },
        false,
        shift,
    )
}

#[cfg(test)]
mod tests {
    use gpui_kit::Modifiers;
    use reframe_core::focus_nav::Direction;
    use reframe_core::shortcuts::TextSizeChange;

    use super::*;

    fn press(key: &str, control: bool, shift: bool) -> Option<Shortcut> {
        shortcut_of(&Keystroke {
            modifiers: Modifiers {
                control,
                shift,
                ..Modifiers::default()
            },
            key: key.to_owned(),
            key_char: None,
        })
    }

    #[test]
    fn keys_are_named_as_the_shortcuts_expect_them() {
        assert_eq!(press("t", true, true), Some(Shortcut::NewSession));
        assert_eq!(press("t", true, false), None);
        assert_eq!(press("3", true, false), Some(Shortcut::FocusTile { index: 2 }));
        assert_eq!(
            press("left", true, true),
            Some(Shortcut::FocusDirection {
                direction: Direction::Left
            })
        );
        assert_eq!(press("space", true, true), Some(Shortcut::NextWaiting));
        assert_eq!(press(",", true, false), Some(Shortcut::OpenSettings));
        assert_eq!(press("f11", false, false), Some(Shortcut::ToggleFullscreen));
        assert_eq!(press("f11", true, false), None);
    }

    #[test]
    fn text_size_keys_go_by_character() {
        let size = |change| Some(Shortcut::TextSize { change });
        assert_eq!(press("=", true, false), size(TextSizeChange::Bigger));
        assert_eq!(press("=", true, true), size(TextSizeChange::Bigger));
        assert_eq!(press("+", true, false), size(TextSizeChange::Bigger));
        assert_eq!(press("-", true, false), size(TextSizeChange::Smaller));
        // Ctrl+Shift+- is Ctrl+_, Claude Code's undo.
        assert_eq!(press("-", true, true), None);
        assert_eq!(press("0", true, false), size(TextSizeChange::Reset));
    }
}
