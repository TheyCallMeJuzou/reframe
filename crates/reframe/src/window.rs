//! A window of reframe: its tiles in the split layout, with separators to drag, group frames,
//! and the strip of collapsed tiles.
//!
//! Takes the place of the Electron version's `src/renderer/App.tsx` and its components
//! (`SplitView`, `Tile`, `GroupFrame`, `DockStrip`). The layout is placed by
//! `reframe_core::geometry` rather than by flex boxes, so the same rectangles serve drawing,
//! dragging and moving the focus.

use std::collections::HashMap;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use reframe_core::actions::LayoutAction;
use reframe_core::focus_nav::{TileBox, neighbor};
use reframe_core::geometry::{Metrics, Placement, Rect, drag_sizes, place, shares};
use reframe_core::i18n::{Locale, t};
use reframe_core::layout::{find_node, leaves, set_split_sizes, tile, tile_key};
use reframe_core::session_logic::{SessionError, SessionStatus, is_running};
use reframe_core::shortcuts::{Shortcut, TextSizeChange};
use reframe_core::snap::{GRID_STEPS, GridAxis, SplitAxis, snap_sizes};
use reframe_core::state::{DockEntry, LayoutNode, SplitDir, WindowState};
use reframe_core::wsl::parse_wsl_path;

use crate::app::{AppEvent, Controller, SessionView};
use crate::dialogs;
use crate::find::Step;
use crate::keys;
use crate::terminal_view::TerminalView;
use crate::theme::{self, icon};

/// A separator being dragged.
struct SeparatorDrag {
    split_id: String,
    index: usize,
    dir: SplitDir,
    /// Where the pointer was along the split when the drag began.
    from: f64,
    /// The split's sizes before the drag, and as they are shown now.
    before: Vec<f64>,
    shown: Vec<f64>,
    /// The split's rectangle and the layout's, for the grid.
    split: Rect,
    area: Rect,
}

/// The field a name is edited in, in place: Enter or leaving the field keeps it, Escape drops
/// the change.
struct Renaming {
    session_id: String,
    initial: String,
    input: Entity<InputState>,
    _events: Subscription,
}

/// The find bar of a tile.
struct FindBar {
    session_id: String,
    input: Entity<InputState>,
    _events: Subscription,
}

pub struct Shell {
    controller: Entity<Controller>,
    window_id: String,
    /// The window this is drawn in.
    handle: AnyWindowHandle,
    /// Has the keyboard while no tile does, so that the shortcuts still arrive.
    focus: FocusHandle,
    /// The view of each session's current start.
    views: HashMap<String, Entity<TerminalView>>,
    /// A session whose terminal gets the keyboard focus with the next frame.
    focus_next: Option<String>,
    /// The session whose terminal had the keyboard last; it gets it back when a dialog closes.
    last_active: Option<String>,
    /// The tile that fills the window for now.
    zoomed: Option<String>,
    drag: Option<SeparatorDrag>,
    renaming: Option<Renaming>,
    find: Option<FindBar>,
    /// What the wheel turned with Ctrl held that did not make a whole step of text size yet.
    wheel: f32,
    /// Where things were placed in the last frame, and the area they were placed in.
    placement: Placement,
    area: Rect,
    _events: Subscription,
}

/// The open windows, by the id of the state's window they show.
#[derive(Default)]
struct Shells(HashMap<String, WeakEntity<Shell>>);

impl Global for Shells {}

/// The window showing one of the state's windows, if it is open.
pub fn shell(window_id: &str, cx: &App) -> Option<Entity<Shell>> {
    cx.try_global::<Shells>()?.0.get(window_id)?.upgrade()
}

fn to_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect::new(
        f64::from(f32::from(bounds.origin.x)),
        f64::from(f32::from(bounds.origin.y)),
        f64::from(f32::from(bounds.size.width)),
        f64::from(f32::from(bounds.size.height)),
    )
}

fn along(dir: SplitDir, position: Point<Pixels>) -> f64 {
    f64::from(f32::from(if dir == SplitDir::Row { position.x } else { position.y }))
}

impl Shell {
    fn new(controller: Entity<Controller>, window_id: String, handle: AnyWindowHandle, cx: &mut Context<Self>) -> Self {
        let events = cx.subscribe(&controller, |this, _, event, cx| match event {
            AppEvent::Session(_) => this.sync(cx),
            AppEvent::Window(id) if *id == this.window_id => {
                // A zoom is over as soon as the layout changes (a tile added, moved, brought back
                // or closed).
                this.zoomed = None;
                this.sync(cx);
            }
            AppEvent::Preferences => cx.notify(),
            _ => {}
        });
        let mut shell = Self {
            controller,
            window_id,
            handle,
            focus: cx.focus_handle(),
            views: HashMap::new(),
            focus_next: None,
            last_active: None,
            zoomed: None,
            drag: None,
            renaming: None,
            find: None,
            wheel: 0.0,
            placement: Placement::default(),
            area: Rect::default(),
            _events: events,
        };
        shell.sync(cx);
        shell
    }

    fn window_state<'a>(&self, cx: &'a App) -> Option<&'a WindowState> {
        self.controller
            .read(cx)
            .state()
            .windows
            .iter()
            .find(|window| window.id == self.window_id)
    }

    fn locale(&self, cx: &App) -> Locale {
        self.controller.read(cx).locale()
    }

    fn text(&self, key: &str, cx: &App) -> String {
        t(self.locale(cx), key, &[]).unwrap_or_else(|| key.to_owned())
    }

    /// Brings the views in step with the sessions: a view per terminal, a new one when a session
    /// was started again.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let tiles = self
            .window_state(cx)
            .map(|window| leaves(window.layout.as_ref()))
            .unwrap_or_default();
        self.views.retain(|id, _| tiles.contains(id));
        // A find bar and a name being edited go with their tile.
        if self.find.as_ref().is_some_and(|find| !tiles.contains(&find.session_id)) {
            self.find = None;
        }
        if self
            .renaming
            .as_ref()
            .is_some_and(|renaming| !tiles.contains(&renaming.session_id))
        {
            self.renaming = None;
        }
        if self.zoomed.as_ref().is_some_and(|id| !tiles.contains(id)) {
            self.zoomed = None;
        }
        for id in tiles {
            let controller = self.controller.read(cx);
            let terminal = controller.sessions().read(cx).terminal(&id);
            let wsl = controller
                .state()
                .sessions
                .get(&id)
                .is_some_and(|record| parse_wsl_path(&record.cwd, None).is_some());
            // An ended session keeps its last screen until it is started again.
            let Some(terminal) = terminal else { continue };
            if self
                .views
                .get(&id)
                .is_none_or(|view| view.read(cx).terminal != terminal)
            {
                let view = cx.new(|cx| TerminalView::new(terminal, wsl, cx));
                self.views.insert(id.clone(), view);
                self.focus_next = Some(id);
            }
        }
        cx.notify();
    }

    fn dispatch(&mut self, action: LayoutAction, cx: &mut Context<Self>) {
        let window_id = self.window_id.clone();
        self.controller
            .update(cx, |controller, cx| controller.dispatch(&window_id, &action, cx));
    }

    /// Where things were placed in the last frame.
    pub fn placement(&self) -> &Placement {
        &self.placement
    }

    pub fn handle(&self) -> AnyWindowHandle {
        self.handle
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn zoomed(&self) -> Option<&str> {
        self.zoomed.as_deref()
    }

    /// Drags a separator the way the pointer would: from its middle by `delta` pixels along its
    /// split, then lets go. For tests.
    pub fn drag_separator(
        &mut self,
        split_id: &str,
        index: usize,
        delta: f32,
        free: bool,
        area: Rect,
        cx: &mut Context<Self>,
    ) {
        let Some(separator) = self.placement.separator(split_id, index).cloned() else {
            return;
        };
        let middle = (
            separator.rect.x + separator.rect.width / 2.0,
            separator.rect.y + separator.rect.height / 2.0,
        );
        let from = point(px(middle.0 as f32), px(middle.1 as f32));
        let to = if separator.dir == SplitDir::Row {
            point(from.x + px(delta), from.y)
        } else {
            point(from.x, from.y + px(delta))
        };
        self.start_drag(split_id, index, from, area, cx);
        self.drag_to(to, free, cx);
        self.end_drag(cx);
    }

    pub fn toggle_zoom(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.zoomed = if self.zoomed.as_deref() == Some(session_id) {
            None
        } else {
            Some(session_id.to_owned())
        };
        self.focus_next = Some(session_id.to_owned());
        cx.notify();
    }

    // ---------- keyboard ----------

    /// The session whose terminal has the keyboard.
    pub fn active_tile(&self, window: &Window, cx: &App) -> Option<String> {
        self.views
            .iter()
            .find(|(_, view)| view.read(cx).focus.contains_focused(window, cx))
            .map(|(id, _)| id.clone())
    }

    fn focus_tile(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.focus_next = Some(session_id);
        cx.notify();
    }

    /// reframe's shortcuts, caught before the terminal (or anything else) sees the key.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Dialogs keep their keys.
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(shortcut) = keys::shortcut_of(&event.keystroke) else {
            return;
        };
        cx.stop_propagation();
        self.run_shortcut(shortcut, window, cx);
    }

    pub fn run_shortcut(&mut self, shortcut: Shortcut, window: &mut Window, cx: &mut Context<Self>) {
        let shown: Vec<String> = self
            .placement
            .tiles
            .iter()
            .map(|tile| tile.session_id.clone())
            .collect();
        let active = self.active_tile(window, cx);
        match shortcut {
            Shortcut::FocusTile { index } => {
                if let Some(id) = shown.get(index) {
                    self.focus_tile(id.clone(), cx);
                }
            }
            Shortcut::FocusDirection { direction } => {
                let Some(from) = active else {
                    if let Some(first) = shown.first() {
                        self.focus_tile(first.clone(), cx);
                    }
                    return;
                };
                let boxes: Vec<TileBox> = self
                    .placement
                    .tiles
                    .iter()
                    .map(|tile| TileBox {
                        id: tile.session_id.clone(),
                        x: tile.rect.x,
                        y: tile.rect.y,
                        width: tile.rect.width,
                        height: tile.rect.height,
                    })
                    .collect();
                if let Some(next) = neighbor(&boxes, &from, direction) {
                    self.focus_tile(next, cx);
                }
            }
            Shortcut::NewSession => self.open_new_session(window, cx),
            Shortcut::OpenSettings => self.open_settings(window, cx),
            Shortcut::Zoom => {
                if let Some(id) = active {
                    self.toggle_zoom(&id, cx);
                }
            }
            Shortcut::ToggleFullscreen => window.toggle_fullscreen(),
            Shortcut::ToggleCollapse => {
                // In a tile: collapse it. Otherwise: bring back the one collapsed last.
                if let Some(session_id) = active {
                    self.dispatch(LayoutAction::TileCollapse { session_id }, cx);
                    return;
                }
                let last = self.window_state(cx).and_then(|window| window.dock.last().cloned());
                match last {
                    Some(DockEntry::Tile { session_id, .. }) => {
                        self.dispatch(
                            LayoutAction::TileExpand {
                                session_id: session_id.clone(),
                            },
                            cx,
                        );
                        self.focus_tile(session_id, cx);
                    }
                    Some(DockEntry::Group { node, .. }) => {
                        self.dispatch(LayoutAction::GroupExpand { group_id: node.id }, cx);
                    }
                    None => {}
                }
            }
            Shortcut::TextSize { change } => self.change_text_size(change, cx),
            Shortcut::Find => {
                // In the focused tile (pressed in the find bar too: the field is selected again).
                let bar = self.find.as_ref().map(|find| find.session_id.clone());
                if let Some(id) = active.or(bar).or_else(|| shown.first().cloned()) {
                    self.open_find(id, window, cx);
                }
            }
            // Windows, the session list and "the next session that needs you" follow with their
            // milestones.
            Shortcut::NewWindow | Shortcut::NextWaiting | Shortcut::ToggleSidebar => {}
        }
    }

    fn change_text_size(&mut self, change: TextSizeChange, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.change_text_size(change, cx));
    }

    /// The wheel over a tile with Ctrl held changes the text size instead of scrolling: a step
    /// per notch, or per that much of a touchpad's movement.
    fn on_wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        if !event.modifiers.control {
            return;
        }
        cx.stop_propagation();
        self.wheel += match event.delta {
            ScrollDelta::Lines(delta) => delta.y.signum() * 100.0,
            ScrollDelta::Pixels(delta) => f32::from(delta.y),
        };
        while self.wheel.abs() >= 100.0 {
            let up = self.wheel > 0.0;
            self.change_text_size(
                if up {
                    TextSizeChange::Bigger
                } else {
                    TextSizeChange::Smaller
                },
                cx,
            );
            self.wheel -= self.wheel.signum() * 100.0;
        }
    }

    // ---------- finding text ----------

    pub fn open_find(&mut self, session_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.as_ref().is_none_or(|find| find.session_id != session_id) {
            self.close_find(cx);
            let placeholder = self.text("find.placeholder", cx);
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let events = cx.subscribe(&input, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Change => this.run_find(Step::Stay, cx),
                InputEvent::PressEnter { shift, .. } => {
                    this.run_find(if *shift { Step::Previous } else { Step::Next }, cx);
                }
                _ => {}
            });
            self.find = Some(FindBar {
                session_id,
                input,
                _events: events,
            });
        }
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// Finds what the bar says in its tile's terminal and goes to a match.
    pub fn run_find(&mut self, step: Step, cx: &mut Context<Self>) {
        let Some(find) = &self.find else { return };
        let text = find.input.read(cx).value();
        if let Some(view) = self.views.get(&find.session_id) {
            view.update(cx, |view, cx| view.find(&text, step, cx));
        }
        cx.notify();
    }

    /// Closes the find bar (and leaves no marks behind); its session is typed in again.
    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        let Some(find) = self.find.take() else { return };
        if let Some(view) = self.views.get(&find.session_id) {
            view.update(cx, |view, cx| view.clear_find(cx));
        }
        self.focus_tile(find.session_id, cx);
    }

    /// What the find bar is at: its text and where the current match is among how many.
    pub fn find_state(&self, cx: &App) -> Option<(String, String, Option<crate::terminal_view::FindResult>)> {
        let find = self.find.as_ref()?;
        let result = self
            .views
            .get(&find.session_id)
            .and_then(|view| view.read(cx).find_result());
        Some((find.session_id.clone(), find.input.read(cx).value().to_string(), result))
    }

    fn find_bar(&self, find: &FindBar, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let locale = self.locale(cx);
        let result = self
            .views
            .get(&find.session_id)
            .and_then(|view| view.read(cx).find_result());
        let count = match result {
            None => String::new(),
            Some(result) if result.count == 0 => self.text("find.none", cx),
            Some(result) => {
                let total = result.count.to_string();
                match result.index {
                    Some(index) => t(
                        locale,
                        "find.count",
                        &[("n", &(index + 1).to_string()), ("total", &total)],
                    ),
                    None => t(locale, "find.found", &[("total", &total)]),
                }
                .unwrap_or_default()
            }
        };
        div()
            .absolute()
            .top(px(6.0))
            .right(px(18.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .py(px(3.0))
            .pl(px(6.0))
            .pr(px(4.0))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgb(theme::BORDER_STRONG))
            .bg(rgb(theme::SURFACE_2))
            .shadow_md()
            // Clicks here stay here (a click in the tile otherwise puts the focus in its terminal).
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_find(cx);
                }
            }))
            .child(div().w(px(180.0)).child(Input::new(&find.input).small()))
            .child(
                div()
                    .min_w(px(64.0))
                    .px(px(6.0))
                    .text_color(rgb(theme::MUTED))
                    .text_size(px(12.0))
                    .text_right()
                    .whitespace_nowrap()
                    .child(count),
            )
            .child(self.icon_button(
                "find-previous",
                icon::UP,
                self.text("find.previous", cx),
                cx,
                |this, _, cx| {
                    this.run_find(Step::Previous, cx);
                },
            ))
            .child(self.icon_button(
                "find-next",
                icon::DOWN,
                self.text("find.next", cx),
                cx,
                |this, _, cx| {
                    this.run_find(Step::Next, cx);
                },
            ))
            .child(self.icon_button(
                "find-close",
                icon::CLOSE,
                self.text("find.close", cx),
                cx,
                |this, _, cx| {
                    this.close_find(cx);
                },
            ))
    }

    // ---------- renaming ----------

    /// Puts a field in place of a session's name.
    pub fn start_rename(&mut self, session_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.controller.read(cx).session_view(&session_id, cx) else {
            return;
        };
        let placeholder = self.text("session.automaticName", cx);
        let initial = view.label;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial.clone())
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let events = cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.finish_rename(true, cx);
            }
        });
        self.renaming = Some(Renaming {
            session_id,
            initial,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// Keeps the name (when it changed) or drops the change; the session is typed in again. Once
    /// kept or dropped, that is it: the field losing the focus afterwards does not keep it again.
    pub fn finish_rename(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else { return };
        let name = renaming.input.read(cx).value().trim().to_owned();
        if keep && name != renaming.initial {
            self.controller.update(cx, |controller, cx| {
                controller.rename_session(&renaming.session_id, &name, cx);
            });
        }
        self.focus_tile(renaming.session_id, cx);
    }

    /// The name being edited, for tests to type into.
    pub fn renaming(&self) -> Option<&str> {
        self.renaming.as_ref().map(|renaming| renaming.session_id.as_str())
    }

    // ---------- dialogs ----------

    pub fn open_new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        dialogs::open_new_session(&self.controller, &self.window_id, window, cx);
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        dialogs::open_settings(&self.controller, window, cx);
    }

    pub fn open_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        dialogs::open_about(&self.controller, window, cx);
    }

    // ---------- separators ----------

    fn start_drag(
        &mut self,
        split_id: &str,
        index: usize,
        position: Point<Pixels>,
        area: Rect,
        cx: &mut Context<Self>,
    ) {
        let Some(separator) = self.placement.separator(split_id, index).cloned() else {
            return;
        };
        let layout = self.window_state(cx).and_then(|window| window.layout.as_ref());
        let Some(LayoutNode::Split(split)) = find_node(layout, split_id) else {
            return;
        };
        // As percentages, whatever they were saved as.
        let before: Vec<f64> = shares(split).iter().map(|share| share * 100.0).collect();
        self.drag = Some(SeparatorDrag {
            split_id: split_id.to_owned(),
            index,
            dir: separator.dir,
            from: along(separator.dir, position),
            shown: before.clone(),
            before,
            split: separator.split,
            area,
        });
        cx.notify();
    }

    /// The pointer moved while a separator is held. `free`: Alt is held, which drags without the
    /// grid.
    fn drag_to(&mut self, position: Point<Pixels>, free: bool, cx: &mut Context<Self>) {
        let snap = self.controller.read(cx).state().preferences.snap_to_grid && !free;
        let Some(drag) = &mut self.drag else { return };
        let metrics = Metrics::default();
        let row = drag.dir == SplitDir::Row;
        let (start, length) = if row {
            (drag.split.x, drag.split.width)
        } else {
            (drag.split.y, drag.split.height)
        };
        let available = length - (drag.before.len() - 1) as f64 * metrics.separator;
        let min = if row { metrics.min_width } else { metrics.min_height };
        let moved = drag_sizes(
            &drag.before,
            drag.index,
            along(drag.dir, position) - drag.from,
            available,
            min,
        );
        drag.shown = if snap {
            let (grid_start, grid_length) = if row {
                (drag.area.x, drag.area.width)
            } else {
                (drag.area.y, drag.area.height)
            };
            let on_grid = snap_sizes(
                &moved,
                &drag.before,
                SplitAxis {
                    start,
                    length,
                    separator: metrics.separator,
                },
                GridAxis {
                    start: grid_start,
                    length: grid_length,
                    steps: GRID_STEPS,
                },
            );
            // A grid line that would leave a panel too small is not taken.
            let fits = on_grid
                .iter()
                .all(|size| size / 100.0 * available >= min.min(available / 100.0) - 0.5);
            let neighbours_fit = [drag.index, drag.index + 1]
                .iter()
                .all(|at| on_grid[*at] / 100.0 * available >= min - 0.5);
            if fits && neighbours_fit { on_grid } else { moved }
        } else {
            moved
        };
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.shown != drag.before {
            self.dispatch(
                LayoutAction::SplitResize {
                    split_id: drag.split_id,
                    sizes: drag.shown,
                },
                cx,
            );
        }
        cx.notify();
    }

    // ---------- building what is drawn ----------

    /// The layout as it is shown now: the zoomed tile alone, or the saved one with the sizes of a
    /// separator that is being dragged.
    fn shown_layout(&self, cx: &App) -> Option<LayoutNode> {
        let layout = self.window_state(cx)?.layout.clone()?;
        if let Some(zoomed) = &self.zoomed {
            return Some(tile(zoomed.clone()));
        }
        match &self.drag {
            Some(drag) => set_split_sizes(Some(&layout), &drag.split_id, &drag.shown).or(Some(layout)),
            None => Some(layout),
        }
    }

    /// The elements of the layout in an area, each with where it goes.
    fn build_layout(&mut self, area: Rect, window: &mut Window, cx: &mut Context<Self>) -> Vec<(Rect, AnyElement)> {
        let layout = self.shown_layout(cx);
        let placement = place(layout.as_ref(), area, &Metrics::default());
        let mut elements = Vec::new();
        let state_layout = self.window_state(cx).and_then(|window| window.layout.clone());
        for group in &placement.groups {
            let Some(LayoutNode::Group(node)) = find_node(state_layout.as_ref(), &group.id) else {
                continue;
            };
            let color = theme::parse_color(&node.color);
            let count = leaves(Some(&node.child)).len();
            let frame = div()
                .size_full()
                .rounded(px(8.0))
                .border_2()
                .border_color(color)
                .bg(color.opacity(0.07))
                .child(
                    div()
                        .h(px(28.0))
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(div().size(px(12.0)).rounded_full().bg(color))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(node.name.clone()))
                        .child(
                            div()
                                .text_color(rgb(theme::MUTED))
                                .text_size(px(12.0))
                                .child(count.to_string()),
                        ),
                );
            elements.push((group.rect, frame.into_any_element()));
        }
        for placed in &placement.tiles {
            let Some(view) = self.controller.read(cx).session_view(&placed.session_id, cx) else {
                continue;
            };
            elements.push((placed.rect, self.tile(&view, window, cx)));
        }
        if self.zoomed.is_none() {
            for separator in &placement.separators {
                let (split_id, index) = (separator.split_id.clone(), separator.index);
                let dragged = self
                    .drag
                    .as_ref()
                    .is_some_and(|drag| drag.split_id == split_id && drag.index == index);
                let handle = div()
                    .id(SharedString::from(format!("separator-{split_id}-{index}")))
                    .size_full()
                    .rounded(px(2.0))
                    .hover(|style| style.bg(rgb(theme::BORDER_STRONG)))
                    .when(dragged, |handle| handle.bg(rgb(theme::BORDER_STRONG)))
                    .map(|handle| {
                        if separator.dir == SplitDir::Row {
                            handle.cursor_col_resize()
                        } else {
                            handle.cursor_row_resize()
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.start_drag(&split_id, index, event.position, area, cx);
                            cx.stop_propagation();
                        }),
                    );
                elements.push((separator.rect, handle.into_any_element()));
            }
        }
        self.placement = placement;
        self.area = area;
        elements
    }

    fn icon_button(
        &self,
        id: impl Into<ElementId>,
        glyph: &'static str,
        hint: String,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .size(px(22.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .text_color(rgb(theme::MUTED))
            .font_family(theme::ICON_FONT)
            .text_size(px(10.0))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme::SURFACE_2)).text_color(rgb(theme::TEXT)))
            .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
            // The header's own mouse handling (focus, later dragging) is not for its buttons.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            .child(glyph)
    }

    fn button(
        &self,
        id: impl Into<ElementId>,
        label: String,
        primary: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .px(px(12.0))
            .py(px(5.0))
            .rounded(px(theme::RADIUS))
            .border_1()
            .cursor_pointer()
            .map(|button| {
                if primary {
                    button
                        .bg(rgb(theme::ACCENT))
                        .border_color(rgb(theme::ACCENT))
                        .text_color(rgb(theme::ACCENT_TEXT))
                        .font_weight(FontWeight::SEMIBOLD)
                } else {
                    button
                        .bg(rgb(theme::SURFACE_2))
                        .border_color(rgb(theme::BORDER_STRONG))
                        .hover(|style| style.border_color(rgb(theme::MUTED)))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            .child(label)
    }

    fn status_dot(&self, status: SessionStatus, cx: &App) -> Stateful<Div> {
        let key = format!("status.{}", crate::sessions::status_name(status));
        let hint = self.text(&key, cx);
        let dot = div().id("status").size(px(8.0)).flex_none().rounded_full();
        let dot = if status == SessionStatus::Ended {
            dot.border_1().border_color(rgb(theme::MUTED))
        } else {
            dot.bg(rgb(theme::status_color(status)))
        };
        dot.tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
    }

    /// A session's tile: its header, its terminal and, without a process, the card saying why.
    fn tile(&mut self, view: &SessionView, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let id = view.id.clone();
        let terminal = self.views.get(&id).cloned();
        let focused = terminal
            .as_ref()
            .is_some_and(|terminal| terminal.read(cx).focus.contains_focused(window, cx));
        let zoomed = self.zoomed.as_deref() == Some(id.as_str());
        let running = is_running(view.status);

        let header = div()
            .h(px(theme::TILE_HEADER))
            .flex_none()
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .bg(rgb(if focused { theme::SURFACE_2 } else { theme::SURFACE }))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                self.icon_button("collapse", icon::COLLAPSE, self.text("tile.collapse", cx), cx, {
                    let id = id.clone();
                    move |this, _, cx| this.dispatch(LayoutAction::TileCollapse { session_id: id.clone() }, cx)
                }),
            )
            .child(self.status_dot(view.status, cx))
            .child(
                match self.renaming.as_ref().filter(|renaming| renaming.session_id == id) {
                    Some(renaming) => div()
                        .w(px(180.0))
                        .flex_none()
                        // Clicks and keys here belong to the field, not to the terminal around it.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.finish_rename(false, cx);
                            }
                        }))
                        .child(Input::new(&renaming.input).small())
                        .into_any_element(),
                    None => {
                        let hint = self.text("session.renameHint", cx);
                        let id = id.clone();
                        div()
                            .id("label")
                            .flex_shrink_0()
                            .max_w(relative(0.4))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(FontWeight::SEMIBOLD)
                            .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                if event.click_count() == 2 {
                                    this.start_rename(id.clone(), window, cx);
                                }
                            }))
                            .child(view.label.clone())
                            .into_any_element()
                    }
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(rgb(theme::MUTED))
                    .text_size(px(12.0))
                    .child(view.cwd_label.clone()),
            )
            .child(self.icon_button(
                "zoom",
                if zoomed { icon::UNZOOM } else { icon::ZOOM },
                self.text(if zoomed { "tile.unzoom" } else { "tile.zoom" }, cx),
                cx,
                {
                    let id = id.clone();
                    move |this, _, cx| this.toggle_zoom(&id, cx)
                },
            ))
            .child(self.icon_button("end", icon::CLOSE, self.text("tile.end", cx), cx, {
                let id = id.clone();
                move |this, _, cx| {
                    this.controller
                        .update(cx, |controller, cx| controller.end_session(&id, cx))
                }
            }));

        let mut body = div().relative().flex_1().min_h_0();
        if let Some(terminal) = terminal {
            body = body.child(
                div()
                    .absolute()
                    .top(px(4.0))
                    .right(px(2.0))
                    .bottom(px(2.0))
                    .left(px(6.0))
                    .child(terminal),
            );
        }
        if !running {
            body = body.child(self.overlay(view, cx));
        }
        if let Some(find) = self.find.as_ref().filter(|find| find.session_id == id) {
            body = body.child(self.find_bar(find, cx));
        }

        div()
            .id(SharedString::from(format!("tile-{id}")))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgb(if focused { theme::ACCENT } else { theme::BORDER }))
            .bg(rgb(theme::TERMINAL))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.focus_next = Some(id.clone());
                    cx.notify();
                }),
            )
            .on_scroll(cx.listener(|this, event, _, cx| this.on_wheel(event, cx)))
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// The card over a session without a process: why, and what can be done about it.
    fn overlay(&self, view: &SessionView, cx: &mut Context<Self>) -> Div {
        let message = match view.error {
            Some(error) => self.text(&format!("tile.error.{}", error.key()), cx),
            None => self.text("tile.ended", cx),
        };
        let mut buttons = div().flex().flex_wrap().justify_center().gap(px(8.0));
        let id = view.id.clone();
        if !matches!(
            view.error,
            Some(SessionError::TranscriptMissing | SessionError::FolderMissing)
        ) {
            let label = if view.error == Some(SessionError::RunningElsewhere) {
                "takeover.confirm"
            } else {
                "tile.resume"
            };
            buttons = buttons.child(self.button("resume", self.text(label, cx), true, cx, {
                let id = id.clone();
                move |this, _, cx| {
                    this.controller
                        .update(cx, |controller, cx| controller.restart_session(&id, false, cx))
                }
            }));
        }
        // A fresh start is for a session whose history is the trouble.
        let fresh = view.error.is_some_and(|error| {
            !matches!(
                error,
                SessionError::FolderMissing | SessionError::HostCrashed | SessionError::RunningElsewhere
            )
        });
        if fresh {
            buttons = buttons.child(self.button("fresh", self.text("tile.startFresh", cx), false, cx, {
                let id = id.clone();
                move |this, _, cx| {
                    this.controller
                        .update(cx, |controller, cx| controller.restart_session(&id, true, cx))
                }
            }));
        }
        buttons = buttons.child(self.button("close", self.text("tile.close", cx), false, cx, {
            move |this, _, cx| {
                this.controller
                    .update(cx, |controller, cx| controller.end_session(&id, cx))
            }
        }));
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p(px(16.0))
            .bg(rgb(theme::TERMINAL).opacity(0.78))
            .child(
                div()
                    .max_w(px(420.0))
                    .px(px(20.0))
                    .py(px(16.0))
                    .rounded(px(theme::RADIUS))
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .bg(rgb(theme::SURFACE))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.0))
                    .child(div().text_center().child(message))
                    .child(buttons),
            )
    }

    /// The bar at the top: a new session at the left; settings and about at the right.
    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let locale = self.locale(cx);
        let new_session = self.text("toolbar.newSession", cx);
        let hint = format!("{new_session} ({})", keys::label(locale, "T", true));
        let settings = format!(
            "{} ({})",
            self.text("settings.title", cx),
            keys::label(locale, ",", false)
        );
        div()
            .h(px(theme::TOOLBAR))
            .flex_none()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .bg(rgb(theme::SURFACE))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                self.button("new-session", new_session, false, cx, |this, window, cx| {
                    this.open_new_session(window, cx);
                })
                .flex()
                .items_center()
                .gap(px(6.0))
                .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx)),
            )
            .child(div().flex_1())
            .when(window.is_fullscreen(), |bar| {
                let hint = format!("{} (F11)", self.text("fullscreen.exit", cx));
                bar.child(
                    self.icon_button("exit-fullscreen", icon::EXIT_FULLSCREEN, hint, cx, |_, window, _| {
                        window.toggle_fullscreen()
                    }),
                )
            })
            .child(
                self.icon_button("settings", icon::SETTINGS, settings, cx, |this, window, cx| {
                    this.open_settings(window, cx);
                })
                .size(px(28.0))
                .text_size(px(14.0)),
            )
            .child(
                self.icon_button(
                    "about",
                    icon::INFO,
                    self.text("about.title", cx),
                    cx,
                    |this, window, cx| this.open_about(window, cx),
                )
                .size(px(28.0))
                .text_size(px(14.0)),
            )
    }

    /// The window's title (and so its taskbar button) says how many of its sessions need the
    /// user.
    pub fn title(&self, cx: &App) -> String {
        let controller = self.controller.read(cx);
        let sessions = controller.sessions().read(cx);
        let waiting = self.window_state(cx).map_or(0, |window| {
            let mut ids = leaves(window.layout.as_ref());
            for entry in &window.dock {
                match entry {
                    DockEntry::Tile { session_id, .. } => ids.push(session_id.clone()),
                    DockEntry::Group { node, .. } => ids.extend(leaves(Some(&node.child))),
                }
            }
            ids.iter()
                .filter(|id| sessions.status(id) == Some(SessionStatus::Waiting))
                .count()
        });
        let locale = controller.locale();
        match waiting {
            0 => "reframe".to_owned(),
            1 => t(locale, "title.waitingOne", &[]).unwrap_or_default(),
            n => t(locale, "title.waiting", &[("n", &n.to_string())]).unwrap_or_default(),
        }
    }

    /// Collapsed tiles and groups; a click puts them back where they were.
    fn dock(&self, entries: &[DockEntry], cx: &mut Context<Self>) -> Div {
        // What a collapsed group shows: the status that most needs attention among its sessions.
        const URGENCY: [SessionStatus; 6] = [
            SessionStatus::Waiting,
            SessionStatus::Error,
            SessionStatus::Busy,
            SessionStatus::Starting,
            SessionStatus::Idle,
            SessionStatus::Ended,
        ];
        let locale = self.locale(cx);
        let mut strip = div()
            .flex_none()
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .px(px(6.0))
            .py(px(4.0))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .bg(rgb(theme::SURFACE));
        let chip = |id: String| {
            div()
                .id(SharedString::from(format!("dock-{id}")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(3.0))
                .max_w(px(220.0))
                .rounded(px(theme::RADIUS))
                .border_1()
                .border_color(rgb(theme::BORDER))
                .bg(rgb(theme::SURFACE_2))
                .text_size(px(12.0))
                .cursor_pointer()
                .hover(|style| style.border_color(rgb(theme::BORDER_STRONG)))
        };
        let label = |text: String| div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(text);
        for entry in entries {
            match entry {
                DockEntry::Tile { session_id, .. } => {
                    let Some(view) = self.controller.read(cx).session_view(session_id, cx) else {
                        continue;
                    };
                    let hint = t(locale, "dock.expand", &[("label", &view.label)]).unwrap_or_default();
                    let id = session_id.clone();
                    strip = strip.child(
                        chip(tile_key(session_id))
                            .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dispatch(LayoutAction::TileExpand { session_id: id.clone() }, cx);
                            }))
                            .child(self.status_dot(view.status, cx))
                            .child(label(view.label)),
                    );
                }
                DockEntry::Group { node, .. } => {
                    let statuses: Vec<SessionStatus> = leaves(Some(&node.child))
                        .iter()
                        .map(|id| {
                            self.controller
                                .read(cx)
                                .session_view(id, cx)
                                .map_or(SessionStatus::Ended, |view| view.status)
                        })
                        .collect();
                    let status = URGENCY
                        .into_iter()
                        .find(|status| statuses.contains(status))
                        .unwrap_or(SessionStatus::Ended);
                    let count = statuses.len().to_string();
                    let hint =
                        t(locale, "dock.expandGroup", &[("label", &node.name), ("count", &count)]).unwrap_or_default();
                    let id = node.id.clone();
                    strip = strip.child(
                        chip(node.id.clone())
                            .border_color(theme::parse_color(&node.color))
                            .border_l_3()
                            .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dispatch(LayoutAction::GroupExpand { group_id: id.clone() }, cx);
                            }))
                            .child(self.status_dot(status, cx))
                            .child(label(node.name.clone()))
                            .child(div().text_color(rgb(theme::MUTED)).text_size(px(11.0)).child(count)),
                    );
                }
            }
        }
        strip
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(view) = self.focus_next.take().and_then(|id| self.views.get(&id)) {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        let (empty, dock) = match self.window_state(cx) {
            Some(state) => (state.layout.is_none(), state.dock.clone()),
            None => (true, Vec::new()),
        };
        window.set_window_title(&self.title(cx));
        if let Some(id) = &self.focus_next {
            self.last_active = Some(id.clone());
        }
        if let Some(active) = self.active_tile(window, cx) {
            self.last_active = Some(active);
        } else if !window.has_active_dialog(cx) && (window.focused(cx).is_none() || self.focus.is_focused(window)) {
            // Nothing has the keyboard (a dialog closed, a tile went): the tile that had it last
            // gets it, else the window itself, so that the shortcuts still arrive.
            match self.last_active.as_ref().and_then(|id| self.views.get(id)) {
                Some(view) if self.focus_next.is_none() => {
                    let focus = view.read(cx).focus.clone();
                    window.focus(&focus, cx);
                }
                _ => window.focus(&self.focus, cx),
            }
        }
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BG))
            .text_color(rgb(theme::TEXT))
            .font_family(theme::UI_FONT)
            .text_size(px(13.0))
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::on_key))
            .child(self.toolbar(window, cx));
        if !dock.is_empty() {
            root = root.child(self.dock(&dock, cx));
        }
        let area = div().flex_1().min_h_0().p(px(theme::LAYOUT_PADDING));
        root.child(if empty {
            area.flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .child(div().text_size(px(18.0)).child(self.text("empty.title", cx)))
                .child(div().text_color(rgb(theme::MUTED)).child(self.text("empty.hint", cx)))
                .child(div().mt(px(10.0)).child(self.button(
                    "empty-new-session",
                    self.text("toolbar.newSession", cx),
                    true,
                    cx,
                    |this, window, cx| this.open_new_session(window, cx),
                )))
        } else {
            area.child(LayoutElement { shell: cx.entity() })
        })
    }
}

/// Draws the layout: asks the shell for the tiles, frames and separators and puts each where
/// the geometry says.
struct LayoutElement {
    shell: Entity<Shell>,
}

impl IntoElement for LayoutElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for LayoutElement {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
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
        let area = to_rect(bounds);
        let built = self.shell.update(cx, |shell, cx| shell.build_layout(area, window, cx));
        let mut elements = Vec::with_capacity(built.len());
        for (rect, mut element) in built {
            let origin = point(px(rect.x as f32), px(rect.y as f32));
            let space = size(
                AvailableSpace::Definite(px(rect.width as f32)),
                AvailableSpace::Definite(px(rect.height as f32)),
            );
            element.prepaint_as_root(origin, space, window, cx);
            elements.push(element);
        }
        elements
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        elements: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for element in elements {
            element.paint(window, cx);
        }
        // While a separator is held, the pointer is followed wherever it goes in the window.
        if self.shell.read(cx).drag.is_some() {
            let shell = self.shell.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    shell.update(cx, |shell, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            shell.drag_to(event.position, event.modifiers.alt, cx);
                        } else {
                            // Released outside the window.
                            shell.end_drag(cx);
                        }
                    });
                    cx.stop_propagation();
                }
            });
            let shell = self.shell.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    shell.update(cx, |shell, cx| shell.end_drag(cx));
                    cx.stop_propagation();
                }
            });
        }
    }
}

/// Opens the window for one of the state's windows. `shown`: on screen, not kept in the tray.
pub fn open(controller: &Entity<Controller>, window_id: &str, shown: bool, cx: &mut App) {
    let options = WindowOptions {
        show: shown,
        // Driven by a test, a window does not take the keyboard from whoever is working.
        focus: shown && !controller.read(cx).config().e2e,
        ..WindowOptions::default()
    };
    let (controller, window_id) = (controller.clone(), window_id.to_owned());
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title("reframe");
        let handle = window.window_handle();
        let shell = cx.new(|cx| Shell::new(controller, window_id.clone(), handle, cx));
        cx.default_global::<Shells>().0.insert(window_id, shell.downgrade());
        shell
    });
    if let Err(error) = opened {
        eprintln!("could not open a window: {error}");
    }
}
