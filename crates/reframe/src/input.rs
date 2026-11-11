//! Turns keys, pastes and mouse actions into the bytes a terminal program expects (the xterm
//! conventions, as xterm.js sends them in the Electron version).

use alacritty_terminal::term::TermMode;
use gpui_kit::{Keystroke, Modifiers};

/// xterm's modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn modifier_param(modifiers: &Modifiers) -> u8 {
    1 + modifiers.shift as u8 + 2 * modifiers.alt as u8 + 4 * modifiers.control as u8
}

/// Bytes for a key that is not plain text. `None` means the key is left to text input (printable
/// characters, AltGr combinations, dead keys, the IME).
pub fn key_bytes(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    let key = keystroke.key.as_str();
    let modified = m.shift || m.alt || m.control;
    let param = modifier_param(m);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    // Cursor keys and Home/End: ESC [ X, ESC O X in application mode, ESC [ 1 ; m X with modifiers.
    let cursor = |letter: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{param}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    // Editing and function keys: ESC [ n ~, ESC [ n ; m ~ with modifiers.
    let tilde = |number: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{number};{param}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    // F1-F4: ESC O X, ESC [ 1 ; m X with modifiers.
    let ss3 = |letter: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{param}{letter}").into_bytes()
        } else {
            format!("\x1bO{letter}").into_bytes()
        }
    };

    let bytes = match key {
        "enter" if m.alt => b"\x1b\r".to_vec(),
        "enter" => b"\r".to_vec(),
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "backspace" if m.alt => b"\x1b\x7f".to_vec(),
        "backspace" if m.control => b"\x08".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3('P'),
        "f2" => ss3('Q'),
        "f3" => ss3('R'),
        "f4" => ss3('S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "space" if m.control => vec![0],
        _ if m.control => {
            // Ctrl+letter and the other C0 controls. Ctrl+Shift+- is Ctrl+_ (Claude Code's undo).
            let byte = match key {
                "-" if m.shift => 0x1f,
                "/" | "_" => 0x1f,
                "[" => 0x1b,
                "\\" => 0x1c,
                "]" => 0x1d,
                "^" => 0x1e,
                _ => {
                    let mut chars = key.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) if c.is_ascii_alphabetic() => c.to_ascii_lowercase() as u8 & 0x1f,
                        _ => return None,
                    }
                }
            };
            if m.alt { vec![0x1b, byte] } else { vec![byte] }
        }
        _ if m.alt => {
            // Alt+character: ESC, then the character.
            let text = keystroke.key_char.as_deref().unwrap_or(key);
            if text.chars().count() != 1 {
                return None;
            }
            let mut bytes = vec![0x1b];
            bytes.extend_from_slice(text.as_bytes());
            bytes
        }
        _ => return None,
    };
    Some(bytes)
}

/// Pasted text: wrapped in the bracketed-paste markers when the program asked for them, otherwise
/// with line ends as Enter.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        // An escape character inside the text could end the bracket early.
        let clean = text.replace('\x1b', "");
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Buttons as the mouse protocols number them.
pub const BUTTON_LEFT: u8 = 0;
pub const BUTTON_MIDDLE: u8 = 1;
pub const BUTTON_RIGHT: u8 = 2;
pub const BUTTON_NONE: u8 = 3;
pub const WHEEL_UP: u8 = 64;
pub const WHEEL_DOWN: u8 = 65;
pub const MOTION: u8 = 32;

/// A mouse report for the cell at `col`/`row` (0-based).
pub fn mouse_report(
    button: u8,
    col: usize,
    row: usize,
    pressed: bool,
    modifiers: &Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let code = button + 4 * modifiers.shift as u8 + 8 * modifiers.alt as u8 + 16 * modifiers.control as u8;
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed { 'M' } else { 'm' };
        Some(format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes())
    } else {
        // The original protocol: one byte each, so it cannot reach past column or row 223.
        if col >= 223 || row >= 223 {
            return None;
        }
        let code = if pressed { code } else { (code & !3) | BUTTON_NONE };
        Some(vec![0x1b, b'[', b'M', 32 + code, 33 + col as u8, 33 + row as u8])
    }
}
