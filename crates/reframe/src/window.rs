//! A window of reframe: its tiles in the split layout, with separators to drag, group frames,
//! and the strip of collapsed tiles.
//!
//! Takes the place of the Electron version's `src/renderer/App.tsx` and its components
//! (`SplitView`, `Tile`, `GroupFrame`, `DockStrip`). The layout is placed by
//! `reframe_core::geometry` rather than by flex boxes, so the same rectangles serve drawing,
//! dragging and moving the focus.

mod sidebar;

use std::collections::HashMap;

use std::sync::atomic::{AtomicUsize, Ordering};

use futures::channel::oneshot;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use reframe_core::actions::{LayoutAction, window_has_session, window_sessions};
use reframe_core::focus_nav::{TileBox, neighbor};
use reframe_core::geometry::{Metrics, Placement, Rect, drag_sizes, place, shares};
use reframe_core::groups::GROUP_COLORS;
use reframe_core::i18n::{Locale, t};
use reframe_core::layout::{DropTarget, find_node, group_of, groups, leaves, set_split_sizes, tile, tile_key};
use reframe_core::session_list::{ListItemRef, OpenMode};
use reframe_core::session_logic::{SessionError, SessionStatus, is_running};
use reframe_core::shortcuts::{Shortcut, TextSizeChange};
use reframe_core::snap::{GRID_STEPS, GridAxis, SplitAxis, snap_sizes};
use reframe_core::state::{CloseWindow, DockEntry, GroupNode, LayoutNode, Side, SplitDir, WindowState};
use reframe_core::wsl::parse_wsl_path;

use crate::app::{AppEvent, Controller, SessionView};
use crate::dialogs;
use crate::find::Step;
use crate::keys;
use crate::terminal_view::TerminalView;
use crate::theme::{self, icon};
use crate::win;

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

/// What a name is edited of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Session(String),
    Group(String),
    /// A session in the session list.
    ListSession(String),
    Workspace(String),
}

/// A tile or a group, as something to drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    Tile,
    Group,
}

/// A tile or group that is being dragged by its bar.
struct NodeDrag {
    key: String,
    kind: DragKind,
    /// What follows the pointer: the session's or the group's name.
    label: String,
    /// Where the button went down, and where the pointer is now.
    from: Point<Pixels>,
    at: Point<Pixels>,
    /// How big it is where it comes from (a window it is torn out into is made to fit).
    size: (f64, f64),
    /// The pointer went far enough for this to be a drag and not a click.
    started: bool,
    /// The other window that shows where a drop would go.
    preview_in: Option<String>,
    /// Rows of the session list are dragged, not a tile or group of the layout.
    list: Option<Vec<ListItemRef>>,
}

/// What a menu entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    RenameSession(String),
    NewGroup(String),
    AddToGroup {
        session: String,
        group: String,
    },
    RemoveFromGroup(String),
    RenameGroup(String),
    RecolorGroup {
        group: String,
        color: String,
    },
    DissolveGroup(String),
    EndGroup(String),
    /// Session list entries opened in this window.
    OpenList {
        refs: Vec<ListItemRef>,
        mode: OpenMode,
    },
    RenameListSession(String),
    SaveWorkspace,
    UpdateWorkspace,
    OpenWorkspace(String),
    RenameWorkspace(String),
    DeleteWorkspace(String),
}

/// A question to the user with one or two answers.
#[derive(Clone, Debug)]
pub struct Question {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub confirm: String,
    /// Without one, there is only the one answer (something that did not work).
    pub cancel: Option<String>,
    /// What it confirms cannot be undone.
    pub danger: bool,
}

/// Asks the user in a window; the answer comes once it is given (no when the window is gone,
/// the dialog was closed, or another question took its place).
pub fn ask(window_id: &str, question: Question, cx: &mut App) -> oneshot::Receiver<bool> {
    let (answer, receiver) = oneshot::channel();
    if let Some(shell) = shell(window_id, cx) {
        let handle = shell.read(cx).handle;
        // Not inside another update of the window: the dialog opens right after.
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.ask(question, answer, window, cx));
            });
        });
    }
    receiver
}

/// An entry of a tile's or group's menu. The menus only describe the choices (as the Electron
/// version's `menus.ts` did for the system's menus); what they do is the window's.
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    /// A dot in a group's colour before the label.
    pub swatch: Option<String>,
    /// A line above it.
    pub separator: bool,
    pub action: Option<MenuAction>,
    /// What a submenu holds.
    pub children: Vec<MenuEntry>,
}

impl MenuEntry {
    fn new(label: String, action: MenuAction) -> Self {
        Self {
            label,
            enabled: true,
            checked: false,
            swatch: None,
            separator: false,
            action: Some(action),
            children: Vec::new(),
        }
    }
}

/// The menu that is open in a window.
struct OpenMenu {
    entries: Vec<MenuEntry>,
    view: Entity<PopupMenu>,
    position: Point<Pixels>,
    _dismissed: Subscription,
}

/// Fills the component library's menu from the entries.
fn menu_items(
    mut menu: PopupMenu,
    entries: &[MenuEntry],
    shell: &WeakEntity<Shell>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for entry in entries {
        if entry.separator {
            menu = menu.separator();
        }
        if !entry.children.is_empty() && entry.enabled {
            let (children, shell) = (entry.children.clone(), shell.clone());
            menu = menu.submenu(entry.label.clone(), window, cx, move |menu, window, cx| {
                menu_items(menu, &children, &shell, window, cx)
            });
            continue;
        }
        let item = match &entry.swatch {
            Some(color) => {
                let (label, color, checked) = (entry.label.clone(), theme::parse_color(color), entry.checked);
                PopupMenuItem::element(move |_, _| {
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().size(px(10.0)).flex_none().rounded_full().bg(color))
                        .child(div().flex_1().child(label.clone()))
                        // The one it has.
                        .when(checked, |row| {
                            row.child(
                                div()
                                    .font_family(theme::ICON_FONT)
                                    .text_size(px(10.0))
                                    .child(icon::CHECK),
                            )
                        })
                })
            }
            None => PopupMenuItem::new(entry.label.clone()),
        };
        let mut item = item.disabled(!entry.enabled);
        if let Some(action) = entry.action.clone() {
            let shell = shell.clone();
            item = item.on_click(move |_, window, cx| {
                // Once the menu is gone: what the entry does may want the keyboard (renaming).
                let (shell, action) = (shell.clone(), action.clone());
                window.defer(cx, move |window, cx| {
                    let _ = shell.update(cx, |shell, cx| shell.run_menu_action(&action, window, cx));
                });
            });
        }
        menu = menu.item(item);
    }
    menu
}

/// The field a name is edited in, in place: Enter or leaving the field keeps it, Escape drops
/// the change.
struct Renaming {
    target: RenameTarget,
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
    menu: Option<OpenMenu>,
    node_drag: Option<NodeDrag>,
    /// Where a tile or group that is being dragged (in this or another window) would go.
    drop_preview: Option<Rect>,
    /// The running session that files are dragged over.
    files_over: Option<String>,
    /// The question whether to end the window's sessions is open.
    asking_to_close: bool,
    /// A question that waits for its answer, and where the answer goes.
    question: Option<(Question, oneshot::Sender<bool>)>,
    /// The session list.
    list: sidebar::ListView,
    /// Closed by the user and out of sight, its sessions running on.
    away: bool,
    /// The window is the one the user works in.
    active: bool,
    /// What Windows knows the window by.
    hwnd: Option<isize>,
    /// When the window was last the active one (a running count): of reframe's windows, the
    /// one that was active last is in front.
    focused_at: usize,
    /// Where the window's content is on the screen.
    screen: Bounds<Pixels>,
    /// How often the window was drawn (keys reach it from the first time on).
    drawn: usize,
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

/// Counts the times a window became the active one.
static FOCUS_COUNT: AtomicUsize = AtomicUsize::new(0);

/// The window showing one of the state's windows, if it is open.
pub fn shell(window_id: &str, cx: &App) -> Option<Entity<Shell>> {
    cx.try_global::<Shells>()?.0.get(window_id)?.upgrade()
}

/// Every open window.
pub fn shells(cx: &App) -> Vec<Entity<Shell>> {
    cx.try_global::<Shells>()
        .map(|shells| shells.0.values().filter_map(WeakEntity::upgrade).collect())
        .unwrap_or_default()
}

/// The window of reframe at a point of the screen that is on screen (not put away), with the
/// point in that window; where windows overlap, the one that was active last. `except`: a window
/// that is not looked at (the one that asks, which cannot be read meanwhile).
pub fn window_at_screen(screen: Point<Pixels>, except: Option<EntityId>, cx: &App) -> Option<(String, Point<Pixels>)> {
    shells(cx)
        .iter()
        .filter(|shell| Some(shell.entity_id()) != except)
        .map(|shell| shell.read(cx))
        .filter(|shell| !shell.away && shell.screen.contains(&screen))
        .max_by_key(|shell| shell.focused_at)
        .map(|shell| (shell.window_id.clone(), screen - shell.screen.origin))
}

/// Opens a window for every one that is added to the state from now on.
pub fn init(controller: &Entity<Controller>, cx: &mut App) {
    cx.subscribe(controller, |controller, event, cx| {
        if let AppEvent::WindowOpened(id) = event {
            open(&controller, id, true, cx);
        }
    })
    .detach();
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
    fn new(
        controller: Entity<Controller>,
        window_id: String,
        shown: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let handle = window.window_handle();
        cx.observe_window_bounds(window, |this, window, cx| this.bounds_changed(window, cx))
            .detach();
        let events = cx.subscribe(&controller, |this, _, event, cx| match event {
            // A session of this window, or one that just left it.
            AppEvent::Session(id) => {
                let here = this.views.contains_key(id)
                    || this
                        .window_state(cx)
                        .is_some_and(|window| window_has_session(window, id));
                if here {
                    this.sync(cx);
                }
            }
            AppEvent::Window(id) if *id == this.window_id => {
                // A zoom is over as soon as the layout changes (a tile added, moved, brought back
                // or closed).
                this.zoomed = None;
                this.sync(cx);
            }
            AppEvent::Preferences | AppEvent::Workspaces => cx.notify(),
            AppEvent::SessionList => this.first_page_changed(cx),
            AppEvent::ShowWindow(id) if *id == this.window_id => this.show(true, cx),
            AppEvent::WindowClosed(id) if *id == this.window_id => this.close_for_good(cx),
            AppEvent::WindowPutAway(id) if *id == this.window_id => this.put_away(cx),
            AppEvent::FocusTile { window, session } if *window == this.window_id => {
                this.focus_tile(session.clone(), cx);
                this.show(true, cx);
            }
            _ => {}
        });
        cx.observe_window_activation(window, |this, window, cx| {
            this.active = window.is_window_active();
            if this.active {
                this.focused_at = FOCUS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                cx.notify();
            }
        })
        .detach();
        let list = sidebar::ListView::new(controller.read(cx).locale(), window, cx);
        let first_page = controller.read(cx).first_page();
        let mut shell = Self {
            list,
            question: None,
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
            menu: None,
            node_drag: None,
            drop_preview: None,
            files_over: None,
            asking_to_close: false,
            away: !shown,
            active: window.is_window_active(),
            hwnd: win::hwnd_of(window),
            focused_at: FOCUS_COUNT.fetch_add(1, Ordering::Relaxed) + 1,
            screen: window.bounds(),
            drawn: 0,
            wheel: 0.0,
            placement: Placement::default(),
            area: Rect::default(),
            _events: events,
        };
        shell.sync(cx);
        match first_page {
            Some(list) => shell.apply_list(list, cx),
            None => shell.reload_list(cx),
        }
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
        let layout = self.window_state(cx).and_then(|window| window.layout.as_ref());
        let state = self.controller.read(cx).state();
        let gone = match self.renaming.as_ref().map(|renaming| &renaming.target) {
            Some(RenameTarget::Session(id)) => !tiles.contains(id),
            Some(RenameTarget::Group(id)) => find_node(layout, id).is_none(),
            Some(RenameTarget::ListSession(id)) => !state.sessions.contains_key(id),
            Some(RenameTarget::Workspace(id)) => !state.workspaces.iter().any(|workspace| workspace.id == *id),
            None => false,
        };
        if gone {
            self.renaming = None;
        }
        // Whatever was being dragged may be gone or somewhere else by now.
        if self
            .node_drag
            .as_ref()
            .is_some_and(|drag| drag.list.is_none() && find_node(layout, &drag.key).is_none())
        {
            self.cancel_node_drag(cx);
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

    /// The window itself or one of its tiles has the keyboard.
    pub fn has_keyboard(&self, window: &Window, cx: &App) -> bool {
        self.focus.is_focused(window) || self.active_tile(window, cx).is_some()
    }

    /// How often the window was drawn.
    pub fn drawn(&self) -> usize {
        self.drawn
    }

    pub fn handle(&self) -> AnyWindowHandle {
        self.handle
    }

    /// The state's window this shows.
    pub fn window_id(&self) -> &str {
        &self.window_id
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

    /// The window was moved, resized, maximized or put on the whole screen: remembered for the
    /// next start.
    fn bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (bounds, maximized, fullscreen) = match window.window_bounds() {
            WindowBounds::Windowed(bounds) => (bounds, false, false),
            WindowBounds::Maximized(bounds) => (bounds, true, false),
            // Full screen is no size of its own: the window keeps the bounds it returns to.
            WindowBounds::Fullscreen(bounds) => (
                bounds,
                self.window_state(cx).is_some_and(|window| window.maximized),
                true,
            ),
        };
        let bounds = reframe_core::state::Bounds {
            x: f64::from(f32::from(bounds.origin.x)).round(),
            y: f64::from(f32::from(bounds.origin.y)).round(),
            width: f64::from(f32::from(bounds.size.width)).round(),
            height: f64::from(f32::from(bounds.size.height)).round(),
        };
        self.screen = window.bounds();
        let window_id = self.window_id.clone();
        self.controller.update(cx, |controller, cx| {
            controller.set_window_bounds(&window_id, bounds, maximized, fullscreen, cx);
        });
        cx.notify();
    }

    // ---------- windows ----------

    /// Whether the window was closed by the user and waits out of sight.
    pub fn is_away(&self) -> bool {
        self.away
    }

    /// A new, empty window next to this one.
    pub fn new_window(&mut self, cx: &mut Context<Self>) -> String {
        let near = self.window_state(cx).and_then(|window| window.bounds);
        self.controller
            .update(cx, |controller, cx| controller.new_window(near, cx))
    }

    /// The user closes the window (its X, Alt+F4). With sessions in it, reframe asks first,
    /// unless an answer was kept.
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.asking_to_close {
            return;
        }
        let choice = self.controller.read(cx).close_choice(&self.window_id);
        match choice {
            Some(choice) => {
                let window_id = self.window_id.clone();
                self.controller.update(cx, |controller, cx| {
                    controller.close_window(&window_id, choice, false, cx);
                });
            }
            None => {
                self.asking_to_close = true;
                let count = self.window_state(cx).map_or(0, |state| window_sessions(state).len());
                dialogs::open_close_window(cx.weak_entity(), &self.controller, count, window, cx);
            }
        }
    }

    /// The answer to "end the sessions in this window?": end them, keep them, or (`None`) leave
    /// the window open. `remember`: do not ask again.
    pub fn answer_close(
        &mut self,
        choice: Option<CloseWindow>,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.asking_to_close {
            return;
        }
        self.asking_to_close = false;
        window.close_dialog(cx);
        if let Some(choice) = choice {
            let window_id = self.window_id.clone();
            self.controller.update(cx, |controller, cx| {
                controller.close_window(&window_id, choice, remember, cx);
            });
        }
    }

    /// Whether the question about closing the window is open.
    pub fn asking_to_close(&self) -> bool {
        self.asking_to_close
    }

    /// The window goes out of sight, into the tray, and its sessions run on.
    pub fn put_away(&mut self, cx: &mut Context<Self>) {
        self.away = true;
        let window_id = self.window_id.clone();
        self.controller.update(cx, |controller, cx| {
            controller.set_window_hidden(&window_id, true, cx);
        });
        if let Some(hwnd) = self.hwnd {
            // Posted: Windows tells the window at once that it is hidden.
            cx.foreground_executor().spawn(async move { win::hide(hwnd) }).detach();
        }
        cx.notify();
    }

    /// The window is on screen again. `activate`: and in front, with the keyboard (not when a
    /// test drives the app: that would take the keyboard from whoever is working).
    pub fn show(&mut self, activate: bool, cx: &mut Context<Self>) {
        self.away = false;
        let window_id = self.window_id.clone();
        self.controller.update(cx, |controller, cx| {
            controller.set_window_hidden(&window_id, false, cx);
        });
        let activate = activate && !self.controller.read(cx).config().e2e;
        let (hwnd, handle) = (self.hwnd, self.handle);
        cx.spawn(async move |_, cx| {
            if let Some(hwnd) = hwnd {
                win::show(hwnd, activate);
            }
            if activate {
                let _ = cx.update(|cx| handle.update(cx, |_, window, _| window.activate_window()));
            }
        })
        .detach();
        cx.notify();
    }

    /// Whether the user is looking at this window right now.
    pub fn is_attentive(&self) -> bool {
        !self.away && self.active
    }

    /// What Windows knows the window by.
    pub fn hwnd(&self) -> Option<isize> {
        self.hwnd
    }

    /// Whether Windows shows the window (minimized counts).
    pub fn is_on_screen(&self) -> bool {
        self.hwnd.is_some_and(win::is_visible)
    }

    /// The window's taskbar button flashes until the window is looked at.
    pub fn flash(&self, cx: &App) {
        if let Some(hwnd) = self.hwnd {
            cx.foreground_executor().spawn(async move { win::flash(hwnd) }).detach();
        }
    }

    /// The window is gone for good (it was a secondary one and nothing is left in it).
    fn close_for_good(&mut self, cx: &mut Context<Self>) {
        let (handle, window_id) = (self.handle, self.window_id.clone());
        cx.defer(move |cx| {
            cx.default_global::<Shells>().0.remove(&window_id);
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
    }

    // ---------- menus ----------

    fn entry(&self, key: &str, action: MenuAction, cx: &App) -> MenuEntry {
        MenuEntry::new(self.text(key, cx), action)
    }

    /// A tile's menu: rename it, and put it into a group or take it out.
    pub fn tile_menu(&self, session_id: &str, cx: &App) -> Vec<MenuEntry> {
        let layout = self.window_state(cx).and_then(|window| window.layout.as_ref());
        let group = group_of(layout, &tile_key(session_id)).map(|group| group.id.clone());
        let session = || session_id.to_owned();
        let others: Vec<MenuEntry> = groups(layout)
            .into_iter()
            .filter(|other| Some(&other.id) != group.as_ref())
            .map(|other| {
                let action = MenuAction::AddToGroup {
                    session: session(),
                    group: other.id.clone(),
                };
                MenuEntry {
                    swatch: Some(other.color.clone()),
                    ..MenuEntry::new(other.name.clone(), action)
                }
            })
            .collect();
        vec![
            self.entry("session.rename", MenuAction::RenameSession(session()), cx),
            MenuEntry {
                separator: true,
                enabled: group.is_none(),
                ..self.entry("menu.newGroup", MenuAction::NewGroup(session()), cx)
            },
            MenuEntry {
                label: self.text("menu.addToGroup", cx),
                enabled: !others.is_empty(),
                checked: false,
                swatch: None,
                separator: false,
                action: None,
                children: others,
            },
            MenuEntry {
                enabled: group.is_some(),
                ..self.entry("menu.removeFromGroup", MenuAction::RemoveFromGroup(session()), cx)
            },
        ]
    }

    /// A group's menu, or (`colors_only`) just the colours it can have.
    pub fn group_menu(&self, group_id: &str, colors_only: bool, cx: &App) -> Vec<MenuEntry> {
        let layout = self.window_state(cx).and_then(|window| window.layout.as_ref());
        let Some(LayoutNode::Group(group)) = find_node(layout, group_id) else {
            return Vec::new();
        };
        let id = || group_id.to_owned();
        let colors: Vec<MenuEntry> = GROUP_COLORS
            .iter()
            .map(|color| {
                let action = MenuAction::RecolorGroup {
                    group: id(),
                    color: color.hex.to_owned(),
                };
                MenuEntry {
                    checked: color.hex == group.color,
                    swatch: Some(color.hex.to_owned()),
                    ..self.entry(color.label, action, cx)
                }
            })
            .collect();
        if colors_only {
            return colors;
        }
        vec![
            self.entry("group.rename", MenuAction::RenameGroup(id()), cx),
            MenuEntry {
                label: self.text("group.color", cx),
                enabled: true,
                checked: false,
                swatch: None,
                separator: false,
                action: None,
                children: colors,
            },
            MenuEntry {
                separator: true,
                ..self.entry("group.dissolve", MenuAction::DissolveGroup(id()), cx)
            },
            self.entry("group.endAll", MenuAction::EndGroup(id()), cx),
        ]
    }

    /// Opens a menu at a place in the window.
    pub fn show_menu(
        &mut self,
        entries: Vec<MenuEntry>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() {
            return;
        }
        let shell = cx.weak_entity();
        let view = PopupMenu::build(window, cx, |menu, window, cx| {
            menu_items(menu, &entries, &shell, window, cx)
        });
        let dismissed = cx.subscribe(&view, |this, _, _: &DismissEvent, cx| {
            this.menu = None;
            cx.notify();
        });
        self.focus_next = None;
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.menu = Some(OpenMenu {
            entries,
            view,
            position,
            _dismissed: dismissed,
        });
        cx.notify();
    }

    /// The open menu's entries.
    pub fn menu(&self) -> Option<&[MenuEntry]> {
        self.menu.as_ref().map(|menu| menu.entries.as_slice())
    }

    /// Picks the entry with that label in the open menu, as a click would. For tests.
    pub fn click_menu_item(&mut self, label: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        fn find<'a>(entries: &'a [MenuEntry], label: &str) -> Option<&'a MenuEntry> {
            entries.iter().find_map(|entry| {
                if entry.label == label && entry.enabled && entry.action.is_some() {
                    Some(entry)
                } else if entry.enabled {
                    find(&entry.children, label)
                } else {
                    None
                }
            })
        }
        let Some(menu) = self.menu.take() else { return false };
        cx.notify();
        match find(&menu.entries, label).and_then(|entry| entry.action.clone()) {
            Some(action) => {
                self.run_menu_action(&action, window, cx);
                true
            }
            None => false,
        }
    }

    fn run_menu_action(&mut self, action: &MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        let controller = self.controller.clone();
        match action {
            MenuAction::RenameSession(session) => self.start_rename(session.clone(), window, cx),
            MenuAction::NewGroup(session) => {
                controller.update(cx, |controller, cx| controller.create_group(session, cx))
            }
            MenuAction::AddToGroup { session, group } => {
                controller.update(cx, |controller, cx| {
                    controller.add_session_to_group(session, group, cx);
                });
            }
            MenuAction::RemoveFromGroup(session) => {
                controller.update(cx, |controller, cx| {
                    controller.remove_session_from_group(session, cx);
                });
            }
            MenuAction::RenameGroup(group) => self.start_group_rename(group.clone(), window, cx),
            MenuAction::RecolorGroup { group, color } => {
                controller.update(cx, |controller, cx| controller.recolor_group(group, color, cx));
            }
            MenuAction::DissolveGroup(group) => {
                controller.update(cx, |controller, cx| controller.dissolve_group(group, cx));
            }
            MenuAction::EndGroup(group) => controller.update(cx, |controller, cx| controller.end_group(group, cx)),
            MenuAction::OpenList { refs, mode } => {
                let window_id = self.window_id.clone();
                controller.update(cx, |controller, cx| {
                    controller.open_sessions(&window_id, refs.clone(), *mode, None, cx);
                });
            }
            MenuAction::RenameListSession(session) => {
                let Some(view) = controller.read(cx).session_view(session, cx) else {
                    return;
                };
                let placeholder = self.text("session.automaticName", cx);
                let target = RenameTarget::ListSession(session.clone());
                self.begin_rename(target, view.label, placeholder, window, cx);
            }
            MenuAction::SaveWorkspace => dialogs::open_save_workspace(&controller, &self.window_id, window, cx),
            MenuAction::UpdateWorkspace => {
                let window_id = self.window_id.clone();
                controller.update(cx, |controller, cx| controller.update_workspace(&window_id, cx));
            }
            MenuAction::OpenWorkspace(workspace) => {
                controller.update(cx, |controller, cx| controller.open_workspace(workspace, cx));
            }
            MenuAction::RenameWorkspace(workspace) => {
                let name = controller
                    .read(cx)
                    .state()
                    .workspaces
                    .iter()
                    .find(|saved| saved.id == *workspace)
                    .map(|saved| saved.name.clone());
                if let Some(name) = name {
                    self.begin_rename(
                        RenameTarget::Workspace(workspace.clone()),
                        name,
                        String::new(),
                        window,
                        cx,
                    );
                }
            }
            MenuAction::DeleteWorkspace(workspace) => {
                let window_id = self.window_id.clone();
                controller.update(cx, |controller, cx| {
                    controller.delete_workspace(&window_id, workspace, cx)
                });
            }
        }
    }

    // ---------- dragging tiles and groups ----------

    /// The key of the tile or group that is being dragged.
    fn dragged(&self) -> Option<&str> {
        self.node_drag
            .as_ref()
            .filter(|drag| drag.started)
            .map(|drag| drag.key.as_str())
    }

    /// The button went down on a tile's or group's bar: a drag begins once the pointer moves.
    fn press_node(&mut self, key: String, kind: DragKind, position: Point<Pixels>, cx: &mut Context<Self>) {
        let renamed = match self.renaming.as_ref().map(|renaming| &renaming.target) {
            Some(RenameTarget::Session(id)) => tile_key(id) == key,
            Some(RenameTarget::Group(id)) => *id == key,
            _ => false,
        };
        if renamed {
            return;
        }
        let found = match kind {
            DragKind::Tile => self
                .placement
                .tiles
                .iter()
                .find(|tile| tile_key(&tile.session_id) == key)
                .and_then(|tile| {
                    let view = self.controller.read(cx).session_view(&tile.session_id, cx)?;
                    Some((tile.rect, view.label))
                }),
            DragKind::Group => self
                .placement
                .groups
                .iter()
                .find(|group| group.id == key)
                .and_then(|group| {
                    let layout = self.window_state(cx).and_then(|window| window.layout.as_ref());
                    match find_node(layout, &key) {
                        Some(LayoutNode::Group(node)) => Some((group.rect, node.name.clone())),
                        _ => None,
                    }
                }),
        };
        let Some((rect, label)) = found else { return };
        self.node_drag = Some(NodeDrag {
            key,
            kind,
            label,
            from: position,
            at: position,
            size: (rect.width, rect.height),
            started: false,
            preview_in: None,
            list: None,
        });
        cx.notify();
    }

    /// The window of reframe under a point of this window (which may lie outside it), in front
    /// of the others, with the point in that window.
    fn window_at(
        &self,
        position: Point<Pixels>,
        me: EntityId,
        window: &Window,
        cx: &App,
    ) -> Option<(String, Point<Pixels>)> {
        let size = window.viewport_size();
        if position.x >= px(0.0) && position.y >= px(0.0) && position.x < size.width && position.y < size.height {
            return Some((self.window_id.clone(), position));
        }
        let screen = window.bounds().origin + position;
        window_at_screen(screen, Some(me), cx)
    }

    /// Where a tile or group dropped at a point of this window would go, and the area to show
    /// for it. Without an area the point takes no drop.
    pub fn drop_spot_at(
        &self,
        position: Point<Pixels>,
        key: &str,
        kind: DragKind,
        cx: &App,
    ) -> (Option<DropTarget>, Option<Rect>) {
        let (x, y) = (f64::from(f32::from(position.x)), f64::from(f32::from(position.y)));
        let empty = self.window_state(cx).is_none_or(|window| window.layout.is_none());
        if empty {
            // An empty window takes it as a whole.
            return (None, self.area.contains(x, y).then_some(self.area));
        }
        let half = |rect: Rect, side: Side| match side {
            Side::Left => Rect::new(rect.x, rect.y, rect.width / 2.0, rect.height),
            Side::Right => Rect::new(rect.x + rect.width / 2.0, rect.y, rect.width / 2.0, rect.height),
            Side::Top => Rect::new(rect.x, rect.y, rect.width, rect.height / 2.0),
            Side::Bottom => Rect::new(rect.x, rect.y + rect.height / 2.0, rect.width, rect.height / 2.0),
        };
        // The edges of a group's frame take what goes next to the group.
        const BAND: f64 = 14.0;
        const OUTSIDE: f64 = 2.0;
        for group in self.placement.groups.iter().filter(|group| group.id != key) {
            let rect = group.rect;
            let (right, bottom) = (rect.x + rect.width, rect.y + rect.height);
            let across = y >= rect.y && y < bottom;
            let along = x >= rect.x && x < right;
            let side = if along && y >= bottom + OUTSIDE - BAND && y < bottom + OUTSIDE {
                Side::Bottom
            } else if along && y >= rect.y - OUTSIDE && y < rect.y - OUTSIDE + BAND {
                Side::Top
            } else if across && x >= right + OUTSIDE - BAND && x < right + OUTSIDE {
                Side::Right
            } else if across && x >= rect.x - OUTSIDE && x < rect.x - OUTSIDE + BAND {
                Side::Left
            } else {
                continue;
            };
            let target = DropTarget {
                key: group.id.clone(),
                side,
            };
            return (Some(target), Some(half(rect, side)));
        }
        for tile in &self.placement.tiles {
            if !tile.rect.contains(x, y) {
                continue;
            }
            let tile_key = tile_key(&tile.session_id);
            // Not onto itself, and a group never lands inside another group.
            let own = tile_key == key || tile.group.as_deref() == Some(key);
            if own || (kind == DragKind::Group && tile.group.is_some()) {
                return (None, None);
            }
            // The nearest edge of the tile's body says on which side.
            let body = tile.rect.inset(0.0, f64::from(theme::TILE_HEADER), 0.0, 0.0);
            let across = ((x - body.x) / body.width.max(1.0)).clamp(0.0, 1.0);
            let down = ((y - body.y) / body.height.max(1.0)).clamp(0.0, 1.0);
            let mut side = (Side::Left, across);
            for other in [
                (Side::Right, 1.0 - across),
                (Side::Top, down),
                (Side::Bottom, 1.0 - down),
            ] {
                if other.1 < side.1 {
                    side = other;
                }
            }
            let target = DropTarget {
                key: tile_key,
                side: side.0,
            };
            return (Some(target), Some(half(body, side.0)));
        }
        (None, None)
    }

    /// Shows where a drop from another window would go (or takes that away).
    pub fn set_drop_preview(&mut self, preview: Option<Rect>, cx: &mut Context<Self>) {
        if self.drop_preview != preview {
            self.drop_preview = preview;
            cx.notify();
        }
    }

    pub fn drop_preview(&self) -> Option<Rect> {
        self.drop_preview
    }

    /// The name that follows the pointer while a tile or group is dragged.
    pub fn dragging(&self) -> Option<&str> {
        self.node_drag
            .as_ref()
            .filter(|drag| drag.started)
            .map(|drag| drag.label.as_str())
    }

    /// The running session that files are dragged over.
    pub fn files_over(&self) -> Option<&str> {
        self.files_over.as_deref()
    }

    fn clear_previews(&mut self, foreign: Option<String>, cx: &mut Context<Self>) {
        self.set_drop_preview(None, cx);
        if let Some(other) = foreign.and_then(|id| shell(&id, cx)) {
            other.update(cx, |other, cx| other.set_drop_preview(None, cx));
        }
    }

    /// The pointer moved with the button held on a tile's or group's bar.
    fn move_node_drag(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.node_drag else { return };
        if !drag.started {
            let moved = position - drag.from;
            if moved.x.abs() < px(5.0) && moved.y.abs() < px(5.0) {
                return;
            }
            drag.started = true;
        }
        drag.at = position;
        let (key, kind, shown_in) = (drag.key.clone(), drag.kind, drag.preview_in.clone());
        let place = self.window_at(position, cx.entity_id(), window, cx);
        let (mut here, mut there) = (None, None);
        match place {
            Some((id, local)) if id == self.window_id => here = self.drop_spot_at(local, &key, kind, cx).1,
            Some((id, local)) => {
                if let Some(other) = shell(&id, cx) {
                    let preview = other.read(cx).drop_spot_at(local, &key, kind, cx).1;
                    other.update(cx, |other, cx| other.set_drop_preview(preview, cx));
                    there = Some(id);
                }
            }
            None => {}
        }
        if shown_in != there
            && let Some(other) = shown_in.and_then(|id| shell(&id, cx))
        {
            other.update(cx, |other, cx| other.set_drop_preview(None, cx));
        }
        if let Some(drag) = &mut self.node_drag {
            drag.preview_in = there;
        }
        self.drop_preview = here;
        cx.notify();
    }

    /// The button was let go: the tile or group goes where the pointer is.
    fn finish_node_drag(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.node_drag.take() else { return };
        self.clear_previews(drag.preview_in.clone(), cx);
        cx.notify();
        if !drag.started {
            return;
        }
        let place = self.window_at(position, cx.entity_id(), window, cx);
        if let Some(refs) = drag.list {
            self.drop_rows(refs, place, cx);
            return;
        }
        let screen = window.bounds().origin + position;
        self.drop_node(&drag.key, drag.kind, place, screen, Some(drag.size), cx);
    }

    /// A drag given up (Escape): nothing moves.
    fn cancel_node_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.node_drag.take() {
            self.clear_previews(drag.preview_in, cx);
            cx.notify();
        }
    }

    /// Lets a tile or group of this window go at a place: in this window it moves, in another
    /// one it goes over there, and outside of reframe's windows (`place` is `None`) it gets a
    /// window of its own at that point of the screen. Says what happened.
    pub fn drop_node(
        &mut self,
        key: &str,
        kind: DragKind,
        place: Option<(String, Point<Pixels>)>,
        screen: Point<Pixels>,
        size: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> &'static str {
        let from = self.window_id.clone();
        match place {
            Some((id, local)) if id == from => {
                let (target, _) = self.drop_spot_at(local, key, kind, cx);
                let Some(target) = target.filter(|target| target.key != key) else {
                    return "nothing";
                };
                self.dispatch(
                    LayoutAction::NodeMove {
                        key: key.to_owned(),
                        target: Some(target),
                    },
                    cx,
                );
                "moved"
            }
            Some((id, local)) => {
                let spot = shell(&id, cx).map(|other| other.read(cx).drop_spot_at(local, key, kind, cx));
                // Only where the other window shows that it takes the drop.
                let Some((target, Some(_))) = spot else {
                    return "nothing";
                };
                let moved = self.controller.update(cx, |controller, cx| {
                    controller.transfer_node(&id, key, &from, target.as_ref(), cx)
                });
                if moved { "transferred" } else { "nothing" }
            }
            None => {
                let point = (f64::from(f32::from(screen.x)), f64::from(f32::from(screen.y)));
                let torn = self
                    .controller
                    .update(cx, |controller, cx| controller.tear_out(&from, key, point, size, cx));
                if torn.is_some() { "tornOut" } else { "nothing" }
            }
        }
    }

    // ---------- files dropped onto a tile ----------

    /// Files are dragged over the window: the running session under the pointer says that it
    /// takes them.
    fn files_hover(&mut self, session_id: &str, inside: bool, cx: &mut Context<Self>) {
        let over = self.files_over.as_deref() == Some(session_id);
        if inside && !over {
            self.files_over = Some(session_id.to_owned());
            cx.notify();
        } else if !inside && over {
            self.files_over = None;
            cx.notify();
        }
    }

    /// Files dropped onto a running session are typed in as their paths, like in a terminal.
    pub fn drop_files(&mut self, session_id: &str, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        self.files_over = None;
        if let Some(view) = self.views.get(session_id) {
            view.update(cx, |view, cx| view.type_paths(paths, cx));
        }
        self.focus_tile(session_id.to_owned(), cx);
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
        // Escape gives up a drag.
        if self.dragged().is_some() && event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.cancel_node_drag(cx);
            return;
        }
        // An open menu keeps its keys.
        if self.menu.is_some() {
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
            Shortcut::NewWindow => {
                self.new_window(cx);
            }
            Shortcut::NextWaiting => {
                let window_id = self.window_id.clone();
                self.controller.update(cx, |controller, cx| {
                    controller.focus_next_waiting(&window_id, active.as_deref(), cx);
                });
            }
            // The session list follows with its milestone.
            Shortcut::ToggleSidebar => self.toggle_sidebar(window, cx),
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
        self.focus_next = None;
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

    fn begin_rename(
        &mut self,
        target: RenameTarget,
        initial: String,
        placeholder: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The field takes the keyboard, also from a tile that was about to get it.
        self.focus_next = None;
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
            target,
            initial,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// Puts a field in place of a session's name.
    pub fn start_rename(&mut self, session_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.controller.read(cx).session_view(&session_id, cx) else {
            return;
        };
        let placeholder = self.text("session.automaticName", cx);
        self.begin_rename(RenameTarget::Session(session_id), view.label, placeholder, window, cx);
    }

    /// Puts a field in place of a group's name.
    pub fn start_group_rename(&mut self, group_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let layout = self.window_state(cx).and_then(|window| window.layout.as_ref());
        let Some(LayoutNode::Group(group)) = find_node(layout, &group_id) else {
            return;
        };
        let name = group.name.clone();
        self.begin_rename(RenameTarget::Group(group_id), name, String::new(), window, cx);
    }

    /// Keeps the name (when it changed) or drops the change; the session is typed in again. Once
    /// kept or dropped, that is it: the field losing the focus afterwards does not keep it again.
    pub fn finish_rename(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else { return };
        let name = renaming.input.read(cx).value().trim().to_owned();
        let changed = keep && name != renaming.initial;
        match renaming.target {
            RenameTarget::Session(session_id) => {
                if changed {
                    self.controller.update(cx, |controller, cx| {
                        controller.rename_session(&session_id, &name, cx);
                    });
                }
                self.focus_tile(session_id, cx);
            }
            RenameTarget::Group(group_id) => {
                // A group always has a name.
                if changed && !name.is_empty() {
                    self.dispatch(LayoutAction::GroupRename { group_id, name }, cx);
                }
                cx.notify();
            }
            RenameTarget::ListSession(session_id) => {
                if changed {
                    self.controller.update(cx, |controller, cx| {
                        controller.rename_session(&session_id, &name, cx);
                    });
                }
                cx.notify();
            }
            RenameTarget::Workspace(workspace_id) => {
                if changed && !name.is_empty() {
                    self.controller.update(cx, |controller, cx| {
                        controller.rename_workspace(&workspace_id, &name, cx);
                    });
                }
                cx.notify();
            }
        }
    }

    // ---------- questions ----------

    /// Opens a question; the answer goes to `answer`. One that was open is answered with no.
    fn ask(&mut self, question: Question, answer: oneshot::Sender<bool>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, previous)) = self.question.take() {
            let _ = previous.send(false);
            window.close_dialog(cx);
        }
        self.question = Some((question.clone(), answer));
        dialogs::open_question(cx.weak_entity(), question, window, cx);
        cx.notify();
    }

    /// The user answered the open question.
    pub fn answer_question(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, answer)) = self.question.take() {
            let _ = answer.send(yes);
            window.close_dialog(cx);
        }
        cx.notify();
    }

    /// The open question, for tests.
    pub fn question(&self) -> Option<&Question> {
        self.question.as_ref().map(|(question, _)| question)
    }

    /// What is being renamed, for tests to type into.
    pub fn renaming(&self) -> Option<&RenameTarget> {
        self.renaming.as_ref().map(|renaming| &renaming.target)
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
            let node = node.clone();
            elements.push((group.rect, self.group_frame(&node, cx)));
        }
        for placed in &placement.tiles {
            let Some(view) = self.controller.read(cx).session_view(&placed.session_id, cx) else {
                continue;
            };
            elements.push((placed.rect, self.tile(&view, placed.group.as_deref(), window, cx)));
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

    /// The field a name is edited in, in a tile's or a group's bar.
    fn name_field(&self, renaming: &Renaming, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(180.0))
            .flex_none()
            // Clicks and keys here belong to the field, not to what is around it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.finish_rename(false, cx);
                }
            }))
            .child(Input::new(&renaming.input).small())
            .into_any_element()
    }

    /// Six dots that say: this bar is what you drag by.
    fn grip(&self) -> Div {
        let dot = || div().size(px(2.0)).rounded_full().bg(rgb(theme::MUTED));
        let column = || {
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(dot())
                .child(dot())
                .child(dot())
        };
        div()
            .flex_none()
            .flex()
            .gap(px(2.0))
            .opacity(0.6)
            .child(column())
            .child(column())
    }

    /// A group's coloured frame with its bar: collapse, colour, name, how many tiles, options.
    fn group_frame(&self, node: &GroupNode, cx: &mut Context<Self>) -> AnyElement {
        let id = node.id.clone();
        let color = theme::parse_color(&node.color);
        let count = leaves(Some(&node.child)).len();
        let target = RenameTarget::Group(id.clone());
        let name = match self.renaming.as_ref().filter(|renaming| renaming.target == target) {
            Some(renaming) => self.name_field(renaming, cx),
            None => {
                let hint = self.text("group.renameHint", cx);
                let id = id.clone();
                div()
                    .id("name")
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.start_group_rename(id.clone(), window, cx);
                        }
                    }))
                    .child(node.name.clone())
                    .into_any_element()
            }
        };
        let color_hint = self.text("group.color", cx);
        let header = div()
            .h(px(28.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let id = id.clone();
                    move |this, event: &MouseDownEvent, _, cx| {
                        this.press_node(id.clone(), DragKind::Group, event.position, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = id.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        let entries = this.group_menu(&id, false, cx);
                        this.show_menu(entries, event.position, window, cx);
                    }
                }),
            )
            .child(
                self.icon_button("collapse", icon::COLLAPSE, self.text("group.collapse", cx), cx, {
                    let id = id.clone();
                    move |this, _, cx| this.dispatch(LayoutAction::GroupCollapse { group_id: id.clone() }, cx)
                }),
            )
            .child(self.grip())
            .child(
                div()
                    .id("color")
                    .size(px(12.0))
                    .flex_none()
                    .rounded_full()
                    .bg(color)
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(color_hint.clone()).build(window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, window, cx| {
                            let entries = this.group_menu(&id, true, cx);
                            this.show_menu(entries, window.mouse_position(), window, cx);
                        }
                    })),
            )
            .child(name)
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::MUTED))
                    .text_size(px(12.0))
                    .child(count.to_string()),
            )
            .child(div().flex_1())
            .child(
                self.icon_button("options", icon::MORE, self.text("group.options", cx), cx, {
                    let id = id.clone();
                    move |this, window, cx| {
                        let entries = this.group_menu(&id, false, cx);
                        this.show_menu(entries, window.mouse_position(), window, cx);
                    }
                }),
            );
        div()
            .id(SharedString::from(format!("group-{id}")))
            .size_full()
            .rounded(px(8.0))
            .border_2()
            .border_color(color)
            .bg(color.opacity(0.07))
            .when(self.dragged() == Some(id.as_str()), |frame| frame.opacity(0.5))
            .child(header)
            .into_any_element()
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
    fn tile(
        &mut self,
        view: &SessionView,
        group: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = view.id.clone();
        // Dragged itself, or with its group.
        let dragged = self
            .dragged()
            .is_some_and(|key| key == tile_key(&id) || Some(key) == group);
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
            // The bar is what a tile is dragged by, and where its menu is.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let id = id.clone();
                    move |this, event: &MouseDownEvent, _, cx| {
                        this.press_node(tile_key(&id), DragKind::Tile, event.position, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = id.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        let entries = this.tile_menu(&id, cx);
                        this.show_menu(entries, event.position, window, cx);
                    }
                }),
            )
            .child(
                self.icon_button("collapse", icon::COLLAPSE, self.text("tile.collapse", cx), cx, {
                    let id = id.clone();
                    move |this, _, cx| this.dispatch(LayoutAction::TileCollapse { session_id: id.clone() }, cx)
                }),
            )
            .child(self.grip())
            .child(self.status_dot(view.status, cx))
            .child(
                match self
                    .renaming
                    .as_ref()
                    .filter(|renaming| renaming.target == RenameTarget::Session(id.clone()))
                {
                    Some(renaming) => self.name_field(renaming, cx),
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
        // Files dropped onto a running session are typed in as their paths.
        if running {
            body = body
                .on_drag_move(cx.listener({
                    let id = id.clone();
                    move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                        this.files_hover(&id, event.bounds.contains(&event.event.position), cx);
                    }
                }))
                .on_drop(cx.listener({
                    let id = id.clone();
                    move |this, paths: &ExternalPaths, _, cx| this.drop_files(&id, paths.paths(), cx)
                }));
            if self.files_over.as_deref() == Some(id.as_str()) {
                body = body.child(
                    div()
                        .absolute()
                        .inset(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(theme::RADIUS))
                        .border_2()
                        .border_dashed()
                        .border_color(rgb(theme::ACCENT))
                        .bg(rgb(theme::TERMINAL).opacity(0.85))
                        .child(self.text("tile.dropFiles", cx)),
                );
            }
        }

        div()
            .id(SharedString::from(format!("tile-{id}")))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .when(dragged, |tile| tile.opacity(0.5))
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
        let new_window = self.text("toolbar.newWindow", cx);
        let window_hint = format!("{new_window} ({})", keys::label(locale, "N", true));
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
            .child({
                let open = self.sidebar_open(cx);
                let hint = format!(
                    "{} ({})",
                    self.text("toolbar.sessions", cx),
                    keys::label(locale, "L", true)
                );
                let dot = self.new_found(cx);
                let dot_hint = self.text("list.newFound", cx);
                self.button(
                    "sessions",
                    self.text("toolbar.sessions", cx),
                    false,
                    cx,
                    |this, window, cx| {
                        this.toggle_sidebar(window, cx);
                    },
                )
                .relative()
                .when(open, |button| {
                    button
                        .bg(rgb(theme::ACCENT).opacity(0.18))
                        .border_color(rgb(theme::ACCENT))
                })
                .tooltip(move |window, cx| {
                    let text = if dot {
                        format!("{hint}\n{dot_hint}")
                    } else {
                        hint.clone()
                    };
                    Tooltip::new(text).build(window, cx)
                })
                .when(dot, |button| {
                    button.child(
                        div()
                            .absolute()
                            .top(px(-3.0))
                            .right(px(-3.0))
                            .size(px(8.0))
                            .rounded_full()
                            .bg(rgb(theme::ACCENT)),
                    )
                })
            })
            .child(
                self.button("new-window", new_window, false, cx, |this, _, cx| {
                    this.new_window(cx);
                })
                .tooltip(move |window, cx| Tooltip::new(window_hint.clone()).build(window, cx)),
            )
            .child({
                let current = {
                    let controller = self.controller.read(cx);
                    self.window_state(cx)
                        .and_then(|window| controller.workspace_of(window))
                        .map(|workspace| workspace.name.clone())
                };
                let label = current.clone().unwrap_or_else(|| self.text("toolbar.workspaces", cx));
                let hint = match &current {
                    Some(name) => format!("{}: {name}", self.text("toolbar.workspaces", cx)),
                    None => self.text("toolbar.workspaces", cx),
                };
                // The menu opens below the button, where it was clicked.
                self.button("workspaces", label, false, cx, |_, _, _| {})
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        let entries = this.workspace_menu(cx);
                        let at = point(event.position().x, px(theme::TOOLBAR - 4.0));
                        this.show_menu(entries, at, window, cx);
                    }))
                    .max_w(px(220.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when(current.is_some(), |button| {
                        button
                            .bg(rgb(theme::ACCENT).opacity(0.18))
                            .border_color(rgb(theme::ACCENT))
                    })
                    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
            })
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
        self.drawn += 1;
        // The question about closing is over however its dialog went (Escape, a click beside it).
        if self.asking_to_close && !window.has_active_dialog(cx) {
            self.asking_to_close = false;
        }
        // A question closed without an answer (Escape, a click beside it) was answered no.
        if self.question.is_some()
            && !window.has_active_dialog(cx)
            && let Some((_, answer)) = self.question.take()
        {
            let _ = answer.send(false);
        }
        // Files that were dragged over a tile left the window again.
        if self.files_over.is_some() && !cx.has_active_drag() {
            self.files_over = None;
        }
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
                // Only when it is not there already: taking the focus draws the window again.
                _ if !self.focus.is_focused(window) => window.focus(&self.focus, cx),
                _ => {}
            }
        }
        let mut root = div()
            .relative()
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
        // The layout element is there without tiles too: it says where the area is, which takes
        // a tile dropped into an empty window.
        let mut area = div()
            .relative()
            .flex_1()
            .min_h_0()
            .h_full()
            .p(px(theme::LAYOUT_PADDING))
            .child(LayoutElement { shell: cx.entity() });
        if empty {
            area = area.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
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
                    ))),
            );
        }
        let mut body = div().flex_1().min_h_0().flex();
        if self.sidebar_open(cx) {
            body = body.child(self.sidebar(window, cx));
        }
        root = root.child(body.child(area.min_w_0()));
        // Over everything: where a drop would go, what is being dragged, and an open menu.
        if let Some(rect) = self.drop_preview {
            root = root.child(
                div()
                    .absolute()
                    .left(px(rect.x as f32))
                    .top(px(rect.y as f32))
                    .w(px(rect.width as f32))
                    .h(px(rect.height as f32))
                    .rounded(px(theme::RADIUS))
                    .border_2()
                    .border_color(rgb(theme::ACCENT))
                    .bg(rgb(theme::ACCENT).opacity(0.22)),
            );
        }
        if let Some(drag) = self.node_drag.as_ref().filter(|drag| drag.started) {
            root = root.child(
                div()
                    .absolute()
                    .left(drag.at.x + px(14.0))
                    .top(drag.at.y + px(16.0))
                    .max_w(px(240.0))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(theme::RADIUS))
                    .border_1()
                    .border_color(rgb(theme::ACCENT))
                    .bg(rgb(theme::SURFACE_2))
                    .shadow_md()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(drag.label.clone()),
            );
        }
        if let Some(menu) = &self.menu {
            root = root.child(
                deferred(
                    anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(px(8.0))
                        .child(menu.view.clone()),
                )
                .with_priority(1),
            );
        }
        root
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
        // While a tile's or group's bar is held, the pointer is followed wherever it goes, also
        // out of the window. A press that never moves stays a click for whatever is under it.
        if self.shell.read(cx).node_drag.is_some() {
            let shell = self.shell.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let started = shell.update(cx, |shell, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            shell.move_node_drag(event.position, window, cx);
                        } else {
                            // Released where the window did not hear of it.
                            shell.finish_node_drag(event.position, window, cx);
                        }
                        shell.dragged().is_some()
                    });
                    if started {
                        cx.stop_propagation();
                    }
                }
            });
            let shell = self.shell.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    let started = shell.update(cx, |shell, cx| {
                        let started = shell.dragged().is_some();
                        shell.finish_node_drag(event.position, window, cx);
                        started
                    });
                    if started {
                        cx.stop_propagation();
                    }
                }
            });
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
    // Where it was, if its top edge is still on a connected display; else only as big as it was.
    let saved = controller
        .read(cx)
        .state()
        .windows
        .iter()
        .find(|window| window.id == window_id)
        .and_then(|window| Some((window.bounds?, window.maximized, window.fullscreen == Some(true))));
    let window_bounds = saved.map(|(saved, maximized, fullscreen)| {
        let size = size(px(saved.width as f32), px(saved.height as f32));
        let title_bar = Bounds::new(
            point(px(saved.x as f32 + 40.0), px(saved.y as f32)),
            gpui_kit::size(px((saved.width as f32 - 80.0).max(1.0)), px(30.0)),
        );
        let on_screen = cx
            .displays()
            .iter()
            .any(|display| display.bounds().intersects(&title_bar));
        let bounds = if on_screen {
            Bounds::new(point(px(saved.x as f32), px(saved.y as f32)), size)
        } else {
            Bounds::centered(None, size, cx)
        };
        if fullscreen {
            WindowBounds::Fullscreen(bounds)
        } else if maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    });
    let options = WindowOptions {
        window_bounds,
        show: shown,
        // Driven by a test, a window does not take the keyboard from whoever is working.
        focus: shown && !controller.read(cx).config().e2e,
        ..WindowOptions::default()
    };
    let (controller, window_id) = (controller.clone(), window_id.to_owned());
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title("reframe");
        let shell = cx.new(|cx| Shell::new(controller, window_id.clone(), shown, window, cx));
        // Closing asks reframe, which ends the sessions or puts the window away.
        window.on_window_should_close(cx, {
            let shell = shell.downgrade();
            move |window, cx| shell.update(cx, |shell, cx| shell.request_close(window, cx)).is_err()
        });
        cx.default_global::<Shells>().0.insert(window_id, shell.downgrade());
        shell
    });
    if let Err(error) = opened {
        eprintln!("could not open a window: {error}");
    }
}
