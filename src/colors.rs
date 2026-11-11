//! The terminal's colours: reframe's background, foreground and selection, and the standard
//! xterm palette for everything else.

use alacritty_terminal::vte::ansi::Rgb;

pub const BACKGROUND: Rgb = Rgb { r: 0x17, g: 0x18, b: 0x1c };
pub const FOREGROUND: Rgb = Rgb { r: 0xd7, g: 0xda, b: 0xe0 };
pub const SELECTION: Rgb = Rgb { r: 0x3a, g: 0x3f, b: 0x4b };

/// xterm's default 16 colours (what xterm.js uses when the theme sets none).
const ANSI: [(u8, u8, u8); 16] = [
    (0x2e, 0x34, 0x36),
    (0xcc, 0x00, 0x00),
    (0x4e, 0x9a, 0x06),
    (0xc4, 0xa0, 0x00),
    (0x34, 0x65, 0xa4),
    (0x75, 0x50, 0x7b),
    (0x06, 0x98, 0x9a),
    (0xd3, 0xd7, 0xcf),
    (0x55, 0x57, 0x53),
    (0xef, 0x29, 0x29),
    (0x8a, 0xe2, 0x34),
    (0xfc, 0xe9, 0x4f),
    (0x72, 0x9f, 0xcf),
    (0xad, 0x7f, 0xa8),
    (0x34, 0xe2, 0xe2),
    (0xee, 0xee, 0xec),
];

/// Colour for an index of alacritty's colour table: 0-15 ANSI, 16-231 the 6x6x6 cube, 232-255
/// greys, 256 foreground, 257 background, 258 cursor, the rest dim/bright variants.
pub fn indexed(index: usize) -> Rgb {
    match index {
        0..=15 => {
            let (r, g, b) = ANSI[index];
            Rgb { r, g, b }
        }
        16..=231 => {
            let i = index - 16;
            let level = |n: usize| if n == 0 { 0 } else { (55 + 40 * n) as u8 };
            Rgb { r: level(i / 36), g: level(i / 6 % 6), b: level(i % 6) }
        }
        232..=255 => {
            let v = (8 + 10 * (index - 232)) as u8;
            Rgb { r: v, g: v, b: v }
        }
        257 => BACKGROUND,
        _ => FOREGROUND,
    }
}
