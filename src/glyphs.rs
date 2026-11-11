//! Box-drawing and block characters drawn as rectangles, so that frames and block art join up
//! from cell to cell whatever the font's own glyphs look like (xterm.js does the same).

use gpui_kit::{Bounds, Hsla, Pixels, Window, fill, point, px};

/// Draws `c` into `cell` if it is one of the characters handled here.
pub fn paint(c: char, cell: Bounds<Pixels>, color: Hsla, window: &mut Window) -> bool {
    let code = c as u32;
    if !(0x2500..=0x259f).contains(&code) {
        return false;
    }

    // Edges snapped to device pixels: neighbouring cells then share exact edges.
    let scale = window.scale_factor();
    let snap = |v: f32| (v * scale).round() / scale;
    let x0 = snap(f32::from(cell.origin.x));
    let y0 = snap(f32::from(cell.origin.y));
    let x1 = snap(f32::from(cell.origin.x + cell.size.width));
    let y1 = snap(f32::from(cell.origin.y + cell.size.height));
    let fx = |f: f32| snap(x0 + (x1 - x0) * f);
    let fy = |f: f32| snap(y0 + (y1 - y0) * f);
    let mut rect = |ax0: f32, ay0: f32, ax1: f32, ay1: f32, color: Hsla| {
        if ax1 > ax0 && ay1 > ay0 {
            window.paint_quad(fill(
                Bounds::from_corners(point(px(ax0), px(ay0)), point(px(ax1), px(ay1))),
                color,
            ));
        }
    };

    match code {
        0x2580 => rect(x0, y0, x1, fy(0.5), color),
        0x2581..=0x2587 => rect(x0, fy(1.0 - (code - 0x2580) as f32 / 8.0), x1, y1, color),
        0x2588 => rect(x0, y0, x1, y1, color),
        0x2589..=0x258f => rect(x0, y0, fx((0x2590 - code) as f32 / 8.0), y1, color),
        0x2590 => rect(fx(0.5), y0, x1, y1, color),
        0x2591..=0x2593 => {
            let alpha = (code - 0x2590) as f32 * 0.25;
            rect(x0, y0, x1, y1, Hsla { a: color.a * alpha, ..color });
        }
        0x2594 => rect(x0, y0, x1, fy(0.125), color),
        0x2595 => rect(fx(0.875), y0, x1, y1, color),
        0x2596..=0x259f => {
            // Quadrants: bit 0 upper left, 1 upper right, 2 lower left, 3 lower right.
            const MASKS: [u8; 10] = [0b0100, 0b1000, 0b0001, 0b1101, 0b1001, 0b0111, 0b1011, 0b0010, 0b0110, 0b1110];
            let mask = MASKS[(code - 0x2596) as usize];
            let (mx, my) = (fx(0.5), fy(0.5));
            if mask & 0b0001 != 0 {
                rect(x0, y0, mx, my, color);
            }
            if mask & 0b0010 != 0 {
                rect(mx, y0, x1, my, color);
            }
            if mask & 0b0100 != 0 {
                rect(x0, my, mx, y1, color);
            }
            if mask & 0b1000 != 0 {
                rect(mx, my, x1, y1, color);
            }
        }
        _ => {
            // Lines: which arms the character has (left, right, up, down) and whether it is heavy.
            let (left, right, up, down, heavy) = match c {
                '─' => (true, true, false, false, false),
                '│' => (false, false, true, true, false),
                '┌' | '╭' => (false, true, false, true, false),
                '┐' | '╮' => (true, false, false, true, false),
                '└' | '╰' => (false, true, true, false, false),
                '┘' | '╯' => (true, false, true, false, false),
                '├' => (false, true, true, true, false),
                '┤' => (true, false, true, true, false),
                '┬' => (true, true, false, true, false),
                '┴' => (true, true, true, false, false),
                '┼' => (true, true, true, true, false),
                '╴' => (true, false, false, false, false),
                '╵' => (false, false, true, false, false),
                '╶' => (false, true, false, false, false),
                '╷' => (false, false, false, true, false),
                '━' => (true, true, false, false, true),
                '┃' => (false, false, true, true, true),
                '┏' => (false, true, false, true, true),
                '┓' => (true, false, false, true, true),
                '┗' => (false, true, true, false, true),
                '┛' => (true, false, true, false, true),
                '┣' => (false, true, true, true, true),
                '┫' => (true, false, true, true, true),
                '┳' => (true, true, false, true, true),
                '┻' => (true, true, true, false, true),
                '╋' => (true, true, true, true, true),
                _ => return false,
            };
            let device = 1.0 / scale;
            let thickness = (snap(if heavy { 2.0 } else { 1.0 })).max(device);
            let cx = snap((x0 + x1 - thickness) / 2.0);
            let cy = snap((y0 + y1 - thickness) / 2.0);
            if left {
                rect(x0, cy, cx + thickness, cy + thickness, color);
            }
            if right {
                rect(cx, cy, x1, cy + thickness, color);
            }
            if up {
                rect(cx, y0, cx + thickness, cy + thickness, color);
            }
            if down {
                rect(cx, cy, cx + thickness, y1, color);
            }
        }
    }
    true
}
