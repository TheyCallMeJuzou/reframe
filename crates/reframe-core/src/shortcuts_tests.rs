use super::Shortcut::*;
use super::TextSizeChange::*;
use super::*;

/// The modifiers held with a key, as words: "ctrl shift" is Ctrl and Shift, "" none.
/// (The Electron version's tests pass an object like `{ ctrl: true, shift: true }`.)
fn keys<'a>(code: &'a str, mods: &str, key: &'a str) -> Keys<'a> {
    let held = |name: &str| mods.split(' ').any(|word| word == name);
    Keys {
        code,
        key,
        ctrl: held("ctrl"),
        meta: held("meta"),
        shift: held("shift"),
        alt: held("alt"),
    }
}

/// What a key press means, where `key`, the character it makes, is the letter or digit of `code`.
fn press(code: &str, mods: &str, mac: bool) -> Option<Shortcut> {
    let key = code
        .strip_prefix("Key")
        .or_else(|| code.strip_prefix("Digit"))
        .unwrap_or(code)
        .to_lowercase();
    shortcut_for(keys(code, mods, &key), mac)
}

/// What a key press means that makes the character `key`.
fn press_key(code: &str, mods: &str, key: &str, mac: bool) -> Option<Shortcut> {
    shortcut_for(keys(code, mods, key), mac)
}

// shortcut_for

#[test]
fn knows_the_ctrl_shift_shortcuts_and_ctrl_1_to_9() {
    assert_eq!(press("KeyT", "ctrl shift", false), Some(NewSession));
    assert_eq!(press("KeyN", "ctrl shift", false), Some(NewWindow));
    assert_eq!(press("KeyM", "ctrl shift", false), Some(ToggleCollapse));
    assert_eq!(press("KeyL", "ctrl shift", false), Some(ToggleSidebar));
    assert_eq!(press_key("Space", "ctrl shift", " ", false), Some(NextWaiting));
    assert_eq!(press_key("Space", "meta shift", " ", true), Some(NextWaiting));
    assert_eq!(press_key("Space", "ctrl", " ", false), None);
    assert_eq!(press("KeyF", "ctrl shift", false), Some(Find));
    assert_eq!(press("KeyZ", "ctrl shift", false), Some(Zoom));
    assert_eq!(
        press("ArrowLeft", "ctrl shift", false),
        Some(FocusDirection {
            direction: Direction::Left
        })
    );
    assert_eq!(press("Digit3", "ctrl", false), Some(FocusTile { index: 2 }));
    assert_eq!(press("F11", "", false), Some(ToggleFullscreen));
    assert_eq!(press_key("Comma", "ctrl", ",", false), Some(OpenSettings));
    assert_eq!(press_key("Comma", "meta", ",", true), Some(OpenSettings));
    assert_eq!(press("F11", "shift", false), None);
}

#[test]
fn leaves_everything_else_to_the_terminal_claude_code_uses_plain_ctrl_keys() {
    assert_eq!(press("KeyT", "ctrl", false), None);
    assert_eq!(press("KeyV", "ctrl shift", false), None);
    assert_eq!(press("KeyT", "ctrl shift alt", false), None);
    assert_eq!(press("KeyT", "shift", false), None);
}

#[test]
fn uses_cmd_instead_of_ctrl_on_macos() {
    assert_eq!(press("KeyT", "meta shift", true), Some(NewSession));
    assert_eq!(press("KeyT", "ctrl shift", true), None);
    assert_eq!(press("Digit1", "meta", false), None);
}

#[test]
fn changes_the_text_size_with_ctrl_and_plus_equals_or_minus_and_0_for_normal_on_any_keyboard_layout() {
    let bigger = Some(TextSize { change: Bigger });
    let smaller = Some(TextSize { change: Smaller });
    assert_eq!(press_key("Equal", "ctrl", "=", false), bigger);
    assert_eq!(press_key("Equal", "ctrl shift", "+", false), bigger); // US
    assert_eq!(press_key("BracketRight", "ctrl", "+", false), bigger); // German
    assert_eq!(press_key("NumpadAdd", "ctrl", "+", false), bigger);
    assert_eq!(press_key("Minus", "ctrl", "-", false), smaller);
    assert_eq!(press_key("Slash", "ctrl", "-", false), smaller); // German
    assert_eq!(press("Digit0", "ctrl", false), Some(TextSize { change: Reset }));
    assert_eq!(press_key("Equal", "meta", "=", true), bigger);
    // Ctrl+_ (Claude Code's undo), Shift+0 (= on a German keyboard) and plain keys stay with the terminal.
    assert_eq!(press_key("Minus", "ctrl shift", "_", false), None);
    assert_eq!(press_key("Digit0", "ctrl shift", "=", false), None);
    assert_eq!(press_key("Minus", "", "-", false), None);
}

// change_font_size and echo_font_size

#[test]
fn changes_at_once_and_takes_a_saved_size_back_only_once_no_change_of_its_own_is_on_its_way() {
    // Ctrl+= twice, then Ctrl+-: the echo of the first press (14) arrives in between and must not set 15 back.
    let mut state = FontSizeState {
        size: 13,
        awaited: None,
    };
    state = change_font_size(state, Bigger).unwrap();
    state = change_font_size(state, Bigger).unwrap();
    assert_eq!(
        state,
        FontSizeState {
            size: 15,
            awaited: Some(15)
        }
    );
    // The Electron version checks for the very same object here; the state is a plain value in Rust.
    assert_eq!(echo_font_size(state, 14), state);
    state = change_font_size(state, Smaller).unwrap();
    assert_eq!(state.size, 14);
    assert_eq!(
        echo_font_size(echo_font_size(state, 15), 14),
        FontSizeState {
            size: 14,
            awaited: None
        }
    );
}

#[test]
fn takes_another_windows_change_when_nothing_is_on_its_way_and_has_no_change_at_the_end_of_the_range() {
    let settled = |size| FontSizeState { size, awaited: None };
    assert_eq!(echo_font_size(settled(14), 20), settled(20));
    assert_eq!(change_font_size(settled(32), Bigger), None);
    assert_eq!(change_font_size(settled(13), Reset), None);
}

// font_size_after

#[test]
fn steps_by_one_within_8_to_32_and_goes_back_to_13() {
    assert_eq!(font_size_after(13, Bigger), 14);
    assert_eq!(font_size_after(13, Smaller), 12);
    assert_eq!(font_size_after(32, Bigger), 32);
    assert_eq!(font_size_after(8, Smaller), 8);
    assert_eq!(font_size_after(20, Reset), 13);
}

// shortcut_label

#[test]
fn writes_shortcuts_the_platform_way() {
    let german = ModifierWords {
        ctrl: "Strg",
        shift: "Umschalt",
    };
    let english = ModifierWords {
        ctrl: "Ctrl",
        shift: "Shift",
    };
    assert_eq!(shortcut_label("T", german, false, true), "Strg+Umschalt+T");
    assert_eq!(shortcut_label("1…9", english, false, false), "Ctrl+1…9");
    assert_eq!(shortcut_label("T", english, true, true), "⌘⇧T");
}
