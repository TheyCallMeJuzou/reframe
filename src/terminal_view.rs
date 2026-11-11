//! A tile: one session drawn as a grid of cells, with keyboard, mouse, selection and clipboard.

use std::ops::Range;
use std::panic::Location;

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{TermMode, viewport_to_point};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use futures::StreamExt;
use gpui_kit::*;

use crate::session::{Launch, Session, TermSize};
use crate::{colors, glyphs, input};

const FONT_FAMILY: &str = "Cascadia Mono";
const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 1.1;
const PADDING: f32 = 4.0;

pub enum TileEvent {
    /// The tile asks to be moved into a window of its own.
    TearOut,
}

/// Where the grid was drawn in the last frame; mouse positions are mapped through it.
#[derive(Clone, Copy, PartialEq)]
struct Layout {
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    cols: usize,
    rows: usize,
}

pub struct TerminalView {
    pub session: Session,
    pub focus: FocusHandle,
    pub title: Option<String>,
    pub exited: bool,
    layout: Option<Layout>,
    /// Text the IME is still composing, shown at the cursor.
    composing: Option<String>,
    selecting: bool,
    /// The mouse button currently reported to the program as held.
    reported_button: Option<u8>,
    reported_cell: Option<(usize, usize)>,
    scroll_remainder: f32,
    focused: bool,
    _events: Task<()>,
}

impl EventEmitter<TileEvent> for TerminalView {}

impl TerminalView {
    pub fn new(launch: &Launch, cx: &mut Context<Self>) -> Self {
        let (session, mut events) = Session::spawn(launch).expect("failed to start the session");
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let mut batch = vec![event];
                while let Ok(event) = events.try_recv() {
                    batch.push(event);
                }
                let alive = this.update(cx, |this, cx| {
                    for event in batch {
                        this.on_term_event(event, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            session,
            focus: cx.focus_handle(),
            title: None,
            exited: false,
            layout: None,
            composing: None,
            selecting: false,
            reported_button: None,
            reported_cell: None,
            scroll_remainder: 0.0,
            focused: false,
            _events: task,
        }
    }

    fn on_term_event(&mut self, event: TermEvent, cx: &mut Context<Self>) {
        match event {
            TermEvent::Title(title) => self.title = Some(title),
            TermEvent::ResetTitle => self.title = None,
            TermEvent::ChildExit(_) | TermEvent::Exit => self.exited = true,
            TermEvent::ClipboardStore(_, text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            _ => {}
        }
        cx.notify();
    }

    /// The middle of a cell in window coordinates, as of the last frame.
    pub fn cell_center(&self, col: usize, row: usize) -> Option<Point<Pixels>> {
        let layout = self.layout?;
        Some(point(
            layout.origin.x + layout.cell.width * (col as f32 + 0.5),
            layout.origin.y + layout.cell.height * (row as f32 + 0.5),
        ))
    }

    /// Columns and rows as of the last frame.
    pub fn grid_size(&self) -> Option<(usize, usize)> {
        self.layout.map(|layout| (layout.cols, layout.rows))
    }

    pub fn selection_text(&self) -> Option<String> {
        self.session.term.lock().selection_to_string()
    }

    pub fn mode(&self) -> TermMode {
        *self.session.term.lock().mode()
    }

    /// Sends input the user produced: the view jumps back to the live screen first.
    fn type_bytes(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        {
            let mut term = self.session.term.lock();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.session.write(bytes);
        cx.notify();
    }

    /// Called while a frame is prepared: keeps the session's size and focus state in step with
    /// what is on screen.
    fn sync(&mut self, layout: Layout, focused: bool) {
        let resized = self.layout.is_none_or(|old| old.cols != layout.cols || old.rows != layout.rows);
        self.layout = Some(layout);
        if resized {
            self.session.resize(
                TermSize { cols: layout.cols as u16, rows: layout.rows as u16 },
                f32::from(layout.cell.width) as u16,
                f32::from(layout.cell.height) as u16,
            );
        }
        if focused != self.focused {
            self.focused = focused;
            if self.mode().contains(TermMode::FOCUS_IN_OUT) {
                self.session.write(if focused { &b"\x1b[I"[..] } else { &b"\x1b[O"[..] });
            }
        }
    }

    // ---- clipboard ----

    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let text = {
            let mut term = self.session.term.lock();
            let text = term.selection_to_string();
            term.selection = None;
            text
        };
        cx.notify();
        match text {
            Some(text) if !text.is_empty() => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                true
            }
            _ => false,
        }
    }

    fn has_selection(&self) -> bool {
        self.session.term.lock().selection.as_ref().is_some_and(|selection| !selection.is_empty())
    }

    /// Text is pasted as text and copied files as their paths. For an image, Claude Code's own
    /// image-paste key (Alt+V) is sent and it reads the clipboard itself.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else { return };
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        if let Some(text) = item.text().filter(|text| !text.is_empty()) {
            self.type_bytes(input::paste_bytes(&text, bracketed), cx);
            return;
        }
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(paths) => {
                    let mut text = String::new();
                    for path in paths.paths() {
                        let path = path.to_string_lossy();
                        if path.contains(|c: char| c.is_whitespace() || "&()^'\"`,;=!%".contains(c)) {
                            text.push_str(&format!("\"{path}\" "));
                        } else {
                            text.push_str(&format!("{path} "));
                        }
                    }
                    self.type_bytes(input::paste_bytes(&text, bracketed), cx);
                    return;
                }
                ClipboardEntry::Image(_) => {
                    self.type_bytes(b"\x1bv".to_vec(), cx);
                    return;
                }
                _ => {}
            }
        }
    }

    // ---- keyboard ----

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let key = keystroke.key.as_str();

        if m.control && !m.alt && !m.platform {
            let handled = match (key, m.shift) {
                ("o", true) => {
                    cx.emit(TileEvent::TearOut);
                    true
                }
                ("d", true) => {
                    self.dump();
                    true
                }
                ("c", true) => {
                    self.copy_selection(cx);
                    true
                }
                // Plain Ctrl+C copies only when something is selected; otherwise it interrupts.
                ("c", false) => self.has_selection() && self.copy_selection(cx),
                ("v", _) => {
                    self.paste(cx);
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
        }

        // AltGr combinations are characters, not Ctrl+Alt shortcuts.
        if event.prefer_character_input {
            return;
        }
        if let Some(bytes) = input::key_bytes(keystroke, self.mode()) {
            self.type_bytes(bytes, cx);
            cx.stop_propagation();
        }
    }

    /// Writes the visible screen to a file in the temp folder (Ctrl+Shift+D).
    fn dump(&self) {
        let file = std::env::temp_dir().join(format!("reframe-native-{}.txt", self.session.pid.unwrap_or(0)));
        match std::fs::write(&file, self.session.screen_text()) {
            Ok(()) => eprintln!("screen written to {}", file.display()),
            Err(err) => eprintln!("could not write {}: {err}", file.display()),
        }
    }

    // ---- mouse ----

    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let layout = self.layout?;
        let x = f32::from(position.x - layout.origin.x) / f32::from(layout.cell.width);
        let y = f32::from(position.y - layout.origin.y) / f32::from(layout.cell.height);
        let col = (x.max(0.0) as usize).min(layout.cols.saturating_sub(1));
        let row = (y.max(0.0) as usize).min(layout.rows.saturating_sub(1));
        let side = if x.fract() < 0.5 || x < 0.0 { Side::Left } else { Side::Right };
        Some((col, row, side))
    }

    /// A program that reads the mouse owns it, except while Shift is held (as in xterm.js and
    /// Windows Terminal).
    fn reports_mouse(&self, modifiers: &Modifiers) -> bool {
        self.mode().intersects(TermMode::MOUSE_MODE) && !modifiers.shift
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some((col, row, side)) = self.cell_at(event.position) else { return };
        let button = match event.button {
            MouseButton::Left => input::BUTTON_LEFT,
            MouseButton::Middle => input::BUTTON_MIDDLE,
            MouseButton::Right => input::BUTTON_RIGHT,
            _ => return,
        };

        if self.reports_mouse(&event.modifiers) {
            let mode = self.mode();
            if let Some(report) = input::mouse_report(button, col, row, true, &event.modifiers, mode) {
                self.session.write(report);
            }
            self.reported_button = Some(button);
            self.reported_cell = Some((col, row));
            return;
        }

        match event.button {
            MouseButton::Left => {
                let kind = match event.click_count {
                    0 | 1 => SelectionType::Simple,
                    2 => SelectionType::Semantic,
                    _ => SelectionType::Lines,
                };
                let mut term = self.session.term.lock();
                let point = viewport_to_point(term.grid().display_offset(), GridPoint::new(row, Column(col)));
                term.selection = Some(Selection::new(kind, point, side));
                drop(term);
                self.selecting = true;
            }
            // Right-click copies the selection, or pastes when there is none.
            MouseButton::Right => {
                if !self.copy_selection(cx) {
                    self.paste(cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, hovered: bool, cx: &mut Context<Self>) {
        let Some((col, row, side)) = self.cell_at(event.position) else { return };

        if self.selecting {
            if event.pressed_button != Some(MouseButton::Left) {
                self.selecting = false;
                return;
            }
            let mut term = self.session.term.lock();
            let point = viewport_to_point(term.grid().display_offset(), GridPoint::new(row, Column(col)));
            if let Some(selection) = term.selection.as_mut() {
                selection.update(point, side);
            }
            drop(term);
            cx.notify();
            return;
        }

        let mode = self.mode();
        let wanted = mode.contains(TermMode::MOUSE_MOTION)
            || (mode.contains(TermMode::MOUSE_DRAG) && self.reported_button.is_some());
        if wanted && (hovered || self.reported_button.is_some()) && !event.modifiers.shift && self.reported_cell != Some((col, row)) {
            self.reported_cell = Some((col, row));
            let button = self.reported_button.unwrap_or(input::BUTTON_NONE) + input::MOTION;
            if let Some(report) = input::mouse_report(button, col, row, true, &event.modifiers, mode) {
                self.session.write(report);
            }
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.selecting && event.button == MouseButton::Left {
            self.selecting = false;
            // A click without a drag selects nothing.
            let mut term = self.session.term.lock();
            if term.selection.as_ref().is_some_and(|selection| selection.is_empty()) {
                term.selection = None;
            }
            drop(term);
            cx.notify();
        }
        if let Some(button) = self.reported_button.take() {
            if let Some((col, row, _)) = self.cell_at(event.position) {
                let mode = self.mode();
                if let Some(report) = input::mouse_report(button, col, row, false, &event.modifiers, mode) {
                    self.session.write(report);
                }
            }
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let Some(layout) = self.layout else { return };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(layout.cell.height),
        };
        self.scroll_remainder += lines;
        let whole = self.scroll_remainder.trunc();
        self.scroll_remainder -= whole;
        let count = whole.abs() as usize;
        if count == 0 {
            return;
        }
        let up = whole > 0.0;
        let mode = self.mode();

        if self.reports_mouse(&event.modifiers) {
            let Some((col, row, _)) = self.cell_at(event.position) else { return };
            let button = if up { input::WHEEL_UP } else { input::WHEEL_DOWN };
            for _ in 0..count {
                if let Some(report) = input::mouse_report(button, col, row, true, &event.modifiers, mode) {
                    self.session.write(report);
                }
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !event.modifiers.shift {
            // A full-screen program without mouse reporting gets arrow keys instead.
            let key: &[u8] = if up { b"\x1bOA" } else { b"\x1bOB" };
            self.session.write(key.repeat(count));
        } else {
            self.session.term.lock().scroll_display(Scroll::Delta(whole as i32));
            cx.notify();
        }
    }
}

// Printable characters, AltGr combinations, dead keys and the IME arrive here as text.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.composing.as_ref().map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composing = None;
        cx.notify();
    }

    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.composing = None;
        if !text.is_empty() {
            self.type_bytes(text.as_bytes().to_vec(), cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composing = (!new_text.is_empty()).then(|| new_text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The IME window goes next to the cursor.
        let layout = self.layout?;
        let cursor = self.session.term.lock().grid().cursor.point;
        let origin = point(
            layout.origin.x + layout.cell.width * cursor.column.0 as f32,
            layout.origin.y + layout.cell.height * cursor.line.0.max(0) as f32,
        );
        Some(Bounds::new(origin, layout.cell))
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .bg(to_hsla(colors::BACKGROUND))
            .p(px(PADDING))
            .child(TerminalElement { view: cx.entity() })
    }
}

fn to_hsla(color: Rgb) -> Hsla {
    Rgba { r: color.r as f32 / 255.0, g: color.g as f32 / 255.0, b: color.b as f32 / 255.0, a: 1.0 }.into()
}

// ---- drawing ----

struct CellPaint {
    c: char,
    extra: Option<String>,
    fg: Hsla,
    bg: Option<Hsla>,
    flags: Flags,
    /// The second half of a wide character, or the filler before one that wrapped.
    spacer: bool,
}

struct CursorPaint {
    col: usize,
    row: usize,
    shape: CursorShape,
}

pub struct Frame {
    hitbox: Hitbox,
    layout: Layout,
    rows: Vec<Vec<CellPaint>>,
    cursor: Option<CursorPaint>,
    focused: bool,
    mouse_mode: bool,
    composing: Option<String>,
}

struct TerminalElement {
    view: Entity<TerminalView>,
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn terminal_font(flags: Flags) -> Font {
    let mut font = font(FONT_FAMILY);
    // Symbols the terminal font lacks come from the monochrome symbol font before the colour emoji font.
    font.fallbacks = Some(FontFallbacks::from_fonts(vec!["Segoe UI Symbol".into()]));
    if flags.contains(Flags::BOLD) {
        font.weight = FontWeight::BOLD;
    }
    if flags.contains(Flags::ITALIC) {
        font.style = FontStyle::Italic;
    }
    font
}

/// The parts of a cell's style that decide whether it can be shaped together with its neighbour.
fn style_flags(flags: Flags) -> Flags {
    flags & (Flags::BOLD | Flags::ITALIC | Flags::ALL_UNDERLINES | Flags::STRIKEOUT | Flags::DIM)
}

fn paint_text(text: String, origin: Point<Pixels>, cell: &CellPaint, line_height: Pixels, window: &mut Window, cx: &mut App) {
    let underline = cell.flags.intersects(Flags::ALL_UNDERLINES).then(|| UnderlineStyle {
        thickness: px(1.0),
        color: Some(cell.fg),
        wavy: cell.flags.contains(Flags::UNDERCURL),
    });
    let strikethrough = cell
        .flags
        .contains(Flags::STRIKEOUT)
        .then(|| StrikethroughStyle { thickness: px(1.0), color: Some(cell.fg) });
    let run = TextRun {
        len: text.len(),
        font: terminal_font(cell.flags),
        color: cell.fg,
        background_color: None,
        underline,
        strikethrough,
    };
    let line = window.text_system().shape_line(text.into(), px(FONT_SIZE), &[run], None);
    let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<Frame>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Cell size from the font: the advance of a character, and its height times the line height.
        let text_system = window.text_system().clone();
        let font_id = text_system.resolve_font(&font(FONT_FAMILY));
        let font_size = px(FONT_SIZE);
        let cell_width = text_system.advance(font_id, font_size, 'm').map(|size| size.width).unwrap_or(px(8.0));
        let glyph_height = f32::from(text_system.ascent(font_id, font_size))
            + f32::from(text_system.descent(font_id, font_size)).abs();
        let cell_height = px((glyph_height * LINE_HEIGHT).ceil());

        let cols = (f32::from(bounds.size.width) / f32::from(cell_width)).floor() as usize;
        let rows = (f32::from(bounds.size.height) / f32::from(cell_height)).floor() as usize;
        if cols < 2 || rows < 2 {
            return None;
        }
        let layout = Layout {
            origin: bounds.origin,
            cell: size(cell_width, cell_height),
            cols: cols.min(u16::MAX as usize),
            rows: rows.min(u16::MAX as usize),
        };

        let focused = self.view.read(cx).focus.is_focused(window) && window.is_window_active();
        self.view.update(cx, |view, _| view.sync(layout, focused));

        let view = self.view.read(cx);
        let term = view.session.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let background = to_hsla(colors::BACKGROUND);
        let selection_color = to_hsla(colors::SELECTION);
        let resolve = |color: Color| -> Rgb {
            match color {
                Color::Spec(rgb) => rgb,
                Color::Named(name) => content.colors[name].unwrap_or_else(|| colors::indexed(name as usize)),
                Color::Indexed(index) => {
                    content.colors[index as usize].unwrap_or_else(|| colors::indexed(index as usize))
                }
            }
        };

        let mut grid: Vec<Vec<CellPaint>> = (0..layout.rows).map(|_| Vec::with_capacity(layout.cols)).collect();
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            if row < 0 || row as usize >= layout.rows {
                continue;
            }
            let cell = indexed.cell;
            let mut fg = to_hsla(resolve(cell.fg));
            let mut bg = (cell.bg != Color::Named(NamedColor::Background)).then(|| to_hsla(resolve(cell.bg)));
            if cell.flags.contains(Flags::INVERSE) {
                let old_fg = fg;
                fg = bg.unwrap_or(background);
                bg = Some(old_fg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg.a *= 0.6;
            }
            if content.selection.is_some_and(|range| range.contains(indexed.point)) {
                bg = Some(selection_color);
            }
            grid[row as usize].push(CellPaint {
                c: cell.c,
                extra: cell.zerowidth().map(|chars| chars.iter().collect()),
                fg,
                bg,
                flags: cell.flags,
                spacer: cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            });
        }

        let cursor_row = content.cursor.point.line.0 + offset;
        let cursor = (content.cursor.shape != CursorShape::Hidden && cursor_row >= 0 && (cursor_row as usize) < layout.rows)
            .then(|| CursorPaint { col: content.cursor.point.column.0, row: cursor_row as usize, shape: content.cursor.shape });
        let mouse_mode = content.mode.intersects(TermMode::MOUSE_MODE);
        drop(term);

        Some(Frame {
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
            layout,
            rows: grid,
            cursor,
            focused,
            mouse_mode,
            composing: view.composing.clone(),
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        frame: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(frame) = frame.take() else { return };
        let layout = frame.layout;
        let (cell_w, cell_h) = (layout.cell.width, layout.cell.height);
        let cell_origin =
            |col: usize, row: usize| point(layout.origin.x + cell_w * col as f32, layout.origin.y + cell_h * row as f32);

        let focus = self.view.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        if !frame.mouse_mode {
            window.set_cursor_style(CursorStyle::IBeam, &frame.hitbox);
        }

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (row, cells) in frame.rows.iter().enumerate() {
                // Backgrounds, one rectangle per run of equal colour.
                let mut col = 0;
                while col < cells.len() {
                    let Some(bg) = cells[col].bg else {
                        col += 1;
                        continue;
                    };
                    let start = col;
                    while col < cells.len() && cells[col].bg == Some(bg) {
                        col += 1;
                    }
                    let origin = cell_origin(start, row);
                    window.paint_quad(fill(Bounds::new(origin, size(cell_w * (col - start) as f32, cell_h)), bg));
                }

                // Text. Runs of plain ASCII in one style are shaped together; every other character
                // is placed on its own cell, so a glyph from a fallback font cannot shift the grid.
                let mut col = 0;
                while col < cells.len() {
                    let cell = &cells[col];
                    let decorated = cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
                    if cell.spacer || cell.flags.contains(Flags::HIDDEN) || (cell.c == ' ' && cell.extra.is_none() && !decorated) {
                        col += 1;
                        continue;
                    }
                    let origin = cell_origin(col, row);
                    if cell.extra.is_none() && glyphs::paint(cell.c, Bounds::new(origin, layout.cell), cell.fg, window) {
                        col += 1;
                        continue;
                    }
                    if cell.c.is_ascii() && cell.extra.is_none() {
                        let mut text = String::new();
                        while col < cells.len() {
                            let next = &cells[col];
                            let same = next.c.is_ascii()
                                && next.extra.is_none()
                                && !next.spacer
                                && !next.flags.contains(Flags::HIDDEN)
                                && next.fg == cell.fg
                                && style_flags(next.flags) == style_flags(cell.flags);
                            if !same {
                                break;
                            }
                            text.push(next.c);
                            col += 1;
                        }
                        if !decorated {
                            text.truncate(text.trim_end().len());
                        }
                        paint_text(text, origin, cell, cell_h, window, cx);
                    } else {
                        let mut text = String::from(cell.c);
                        if let Some(extra) = &cell.extra {
                            text.push_str(extra);
                        }
                        paint_text(text, origin, cell, cell_h, window, cx);
                        col += 1;
                    }
                }
            }

            // Cursor: a solid shape while the tile has the focus, an outline otherwise.
            if let Some(cursor) = &frame.cursor {
                let color = to_hsla(colors::FOREGROUND);
                let origin = cell_origin(cursor.col, cursor.row);
                let cell_bounds = Bounds::new(origin, layout.cell);
                if !frame.focused || cursor.shape == CursorShape::HollowBlock {
                    window.paint_quad(quad(cell_bounds, px(0.0), transparent_black(), px(1.0), color, BorderStyle::default()));
                } else {
                    match cursor.shape {
                        CursorShape::Beam => window.paint_quad(fill(Bounds::new(origin, size(px(2.0), cell_h)), color)),
                        CursorShape::Underline => window.paint_quad(fill(
                            Bounds::new(point(origin.x, origin.y + cell_h - px(2.0)), size(cell_w, px(2.0))),
                            color,
                        )),
                        _ => {
                            window.paint_quad(fill(cell_bounds, color));
                            // The character under a block cursor is drawn again in the background colour.
                            if let Some(cell) = frame.rows.get(cursor.row).and_then(|cells| cells.get(cursor.col)) {
                                if cell.c != ' ' && !cell.spacer {
                                    let inverted = CellPaint {
                                        c: cell.c,
                                        extra: None,
                                        fg: to_hsla(colors::BACKGROUND),
                                        bg: None,
                                        flags: cell.flags,
                                        spacer: false,
                                    };
                                    paint_text(cell.c.to_string(), origin, &inverted, cell_h, window, cx);
                                }
                            }
                        }
                    }
                }

                // Text the IME is composing, underlined at the cursor.
                if let Some(text) = &frame.composing {
                    let width = cell_w * text.chars().count() as f32;
                    window.paint_quad(fill(Bounds::new(origin, size(width, cell_h)), to_hsla(colors::BACKGROUND)));
                    let composing = CellPaint {
                        c: ' ',
                        extra: None,
                        fg: color,
                        bg: None,
                        flags: Flags::UNDERLINE,
                        spacer: false,
                    };
                    paint_text(text.clone(), origin, &composing, cell_h, window, cx);
                }
            }
        });

        // Mouse. Presses and the wheel count only over the tile; moves and releases are followed
        // anywhere, so a drag can leave the tile.
        let view = self.view.clone();
        let hitbox = frame.hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                view.update(cx, |view, cx| view.on_mouse_down(event, window, cx));
            }
        });
        let view = self.view.clone();
        let hitbox = frame.hitbox.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                let hovered = hitbox.is_hovered(window);
                view.update(cx, |view, cx| view.on_mouse_move(event, hovered, cx));
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| view.on_mouse_up(event, cx));
            }
        });
        let view = self.view.clone();
        let hitbox = frame.hitbox.clone();
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                view.update(cx, |view, cx| view.on_scroll(event, cx));
            }
        });
    }
}
