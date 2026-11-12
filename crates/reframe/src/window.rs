//! A window of reframe. For now its sessions side by side, each with its terminal or, without a
//! process, a line saying why. The real window (split tree, tile headers, dock, dialogs) follows
//! with the window milestone.

use std::collections::HashMap;

use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::*;
use reframe_core::layout::leaves;
use reframe_core::wsl::parse_wsl_path;

use crate::app::{AppEvent, Controller};
use crate::sessions::status_name;
use crate::terminal_view::TerminalView;

pub struct Shell {
    controller: Entity<Controller>,
    window_id: String,
    /// The view of each session's current start.
    views: HashMap<String, Entity<TerminalView>>,
    /// A session whose terminal gets the keyboard focus with the next frame.
    focus_next: Option<String>,
    _events: Subscription,
}

impl Shell {
    fn new(controller: Entity<Controller>, window_id: String, cx: &mut Context<Self>) -> Self {
        let events = cx.subscribe(&controller, |this, _, event, cx| match event {
            AppEvent::Session(_) => this.sync(cx),
            AppEvent::Window(id) if *id == this.window_id => this.sync(cx),
            _ => {}
        });
        let mut shell = Self {
            controller,
            window_id,
            views: HashMap::new(),
            focus_next: None,
            _events: events,
        };
        shell.sync(cx);
        shell
    }

    /// The sessions with a tile in this window, in the order of the layout.
    fn tiles(&self, cx: &App) -> Vec<String> {
        let state = self.controller.read(cx).state();
        let window = state.windows.iter().find(|window| window.id == self.window_id);
        window.map(|window| leaves(window.layout.as_ref())).unwrap_or_default()
    }

    /// Brings the views in step with the sessions: a view per terminal, a new one when a session
    /// was started again.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let tiles = self.tiles(cx);
        self.views.retain(|id, _| tiles.contains(id));
        for id in tiles {
            let controller = self.controller.read(cx);
            let terminal = controller.sessions().read(cx).terminal(&id);
            let wsl = controller
                .state()
                .sessions
                .get(&id)
                .is_some_and(|record| parse_wsl_path(&record.cwd, None).is_some());
            match terminal {
                Some(terminal)
                    if self
                        .views
                        .get(&id)
                        .is_none_or(|view| view.read(cx).terminal != terminal) =>
                {
                    let view = cx.new(|cx| TerminalView::new(terminal, wsl, cx));
                    self.views.insert(id.clone(), view);
                    self.focus_next = Some(id);
                }
                // An ended session keeps its last screen until it is started again.
                _ => {}
            }
        }
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(view) = self.focus_next.take().and_then(|id| self.views.get(&id)) {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        let tiles = self.tiles(cx);
        let root = div().size_full().bg(rgb(0x17181c)).text_color(rgb(0x9aa0aa));
        if tiles.is_empty() {
            return root.flex().items_center().justify_center().child("No sessions");
        }
        let mut group = h_resizable("tiles");
        for id in &tiles {
            let panel = match self.views.get(id) {
                Some(view) => resizable_panel().child(view.clone()),
                None => {
                    let sessions = self.controller.read(cx).sessions().read(cx);
                    let line = match (sessions.error(id), sessions.status(id)) {
                        (Some(error), _) => format!("not started: {}", error.key()),
                        (None, Some(status)) => status_name(status).to_owned(),
                        (None, None) => "ended".to_owned(),
                    };
                    resizable_panel().child(div().size_full().flex().items_center().justify_center().child(line))
                }
            };
            group = group.child(panel);
        }
        root.child(group)
    }
}

/// Opens the window for one of the state's windows. `shown`: on screen (tests keep them hidden).
pub fn open(controller: &Entity<Controller>, window_id: &str, shown: bool, cx: &mut App) {
    let options = WindowOptions {
        show: shown,
        focus: shown,
        ..WindowOptions::default()
    };
    let (controller, window_id) = (controller.clone(), window_id.to_owned());
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title("reframe");
        cx.new(|cx| Shell::new(controller, window_id, cx))
    });
    if let Err(error) = opened {
        eprintln!("could not open a window: {error}");
    }
}
