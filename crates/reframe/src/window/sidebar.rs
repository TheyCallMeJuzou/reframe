//! The session list at the window's left: saved workspaces, reframe's sessions, and sessions
//! found outside it (running or past), with search and "Show more". A click opens an entry;
//! Shift+click selects a range and Ctrl+click single entries, to open them together from the
//! right-click menu or by dragging them into the layout.
//!
//! Takes the place of the Electron version's `SessionSidebar.tsx` and `session-list.ts`.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use reframe_core::actions::LayoutAction;
use reframe_core::i18n::{Locale, t};
use reframe_core::layout::tile_key;
use reframe_core::session_list::{
    FreshTracker, LIST_PAGE, ListItemRef, OpenMode, SessionList, exact_time, folder_name, short_time,
};
use reframe_core::session_logic::SessionStatus;

use super::{DragKind, MenuAction, MenuEntry, NodeDrag, RenameTarget, Shell};
use crate::app::local_time;
use crate::theme::{self, icon};

/// How long a new entry stays highlighted once the list is shown.
const HIGHLIGHT: Duration = Duration::from_millis(4000);
/// How often "5m" moves on while the list stays the same.
const TICK: Duration = Duration::from_millis(30_000);
/// A moment's wait while typing in the search box.
const TYPING: Duration = Duration::from_millis(150);
pub const WIDTH: f32 = 270.0;
/// The layers glyph of the icon font, for workspaces.
const LAYERS: &str = "\u{E81E}";

/// The session list's state in a window.
pub struct ListView {
    pub search: Entity<InputState>,
    /// The language the search box's placeholder is in.
    search_locale: Locale,
    query: String,
    limit: usize,
    /// What the list shows.
    list: Option<Rc<SessionList>>,
    fresh: FreshTracker,
    /// The selected rows' keys, and where a Shift+click range starts.
    selected: Vec<String>,
    anchor: Option<String>,
    loading: Option<Task<()>>,
    highlight: Option<Task<()>>,
    _ticker: Task<()>,
    _search: Subscription,
}

/// The search box, with its placeholder in the language, and what typing in it does.
fn search_box(
    value: &str,
    locale: Locale,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> (Entity<InputState>, Subscription) {
    let placeholder = t(locale, "list.search", &[]).unwrap_or_default();
    let search = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value.to_owned())
    });
    let subscription = cx.subscribe(&search, |shell, search, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            shell.list.query = search.read(cx).value().to_string();
            shell.list.limit = LIST_PAGE;
            shell.reload_list(cx);
        }
    });
    (search, subscription)
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_millis() as f64)
}

impl ListView {
    pub fn new(locale: Locale, window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let (search, subscription) = search_box("", locale, window, cx);
        let ticker = cx.spawn(async move |shell, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let shown = shell.update(cx, |shell, cx| {
                    if shell.sidebar_open(cx) {
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    break;
                }
            }
        });
        Self {
            search,
            search_locale: locale,
            query: String::new(),
            limit: LIST_PAGE,
            list: None,
            fresh: FreshTracker::new(now_ms()),
            selected: Vec::new(),
            anchor: None,
            loading: None,
            highlight: None,
            _ticker: ticker,
            _search: subscription,
        }
    }

    /// A search or more pages are asked for only while the list is shown; hidden, the first page
    /// is enough (for the toolbar's dot).
    fn first_page_only(&self, shown: bool) -> bool {
        !shown || (self.query.trim().is_empty() && self.limit == LIST_PAGE)
    }
}

impl Shell {
    pub fn sidebar_open(&self, cx: &App) -> bool {
        self.window_state(cx)
            .is_some_and(|window| window.sidebar_open == Some(true))
    }

    /// Shows the list or takes it away; shown, the search box gets the keyboard.
    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.sidebar_open(cx);
        self.dispatch(LayoutAction::WindowSidebar { open }, cx);
        if open {
            self.focus_next = None;
            self.list.search.update(cx, |search, cx| search.focus(window, cx));
            self.reload_list(cx);
        }
    }

    /// The controller made a new first page.
    pub(super) fn first_page_changed(&mut self, cx: &mut Context<Self>) {
        if self.list.first_page_only(self.sidebar_open(cx)) {
            if let Some(list) = self.controller.read(cx).first_page() {
                self.apply_list(list, cx);
            }
        } else {
            self.reload_list(cx);
        }
    }

    /// Asks for the list as this window wants it (searched, more pages).
    pub fn reload_list(&mut self, cx: &mut Context<Self>) {
        let shown = self.sidebar_open(cx);
        let (query, limit) = if shown {
            (self.list.query.clone(), self.list.limit)
        } else {
            (String::new(), LIST_PAGE)
        };
        let controller = self.controller.clone();
        self.list.loading = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(TYPING).await;
            let list = controller
                .update(cx, |controller, cx| controller.session_list(query, limit, cx))
                .await;
            let _ = shell.update(cx, |shell, cx| shell.apply_list(Rc::new(list), cx));
        }));
    }

    pub(super) fn apply_list(&mut self, list: Rc<SessionList>, cx: &mut Context<Self>) {
        let popped = self.list.fresh.apply(&list);
        self.list.list = Some(list);
        if popped || (self.sidebar_open(cx) && !self.list.fresh.fresh().is_empty()) {
            self.highlight_fresh(cx);
        }
        cx.notify();
    }

    /// Once the list is shown, new entries light up for a moment and then count as seen.
    fn highlight_fresh(&mut self, cx: &mut Context<Self>) {
        if !self.sidebar_open(cx) || self.list.highlight.is_some() {
            return;
        }
        self.list.highlight = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(HIGHLIGHT).await;
            let _ = shell.update(cx, |shell, cx| {
                shell.list.fresh.clear();
                shell.list.highlight = None;
                cx.notify();
            });
        }));
    }

    /// Whether new entries wait to be seen (the toolbar's dot).
    pub fn new_found(&self, cx: &App) -> bool {
        !self.sidebar_open(cx) && !self.list.fresh.fresh().is_empty()
    }

    /// What the list shows now, for tests.
    pub fn shown_list(&self) -> Option<Rc<SessionList>> {
        self.list.list.clone()
    }

    pub fn selected_rows(&self) -> &[String] {
        &self.list.selected
    }

    fn rows(&self) -> Vec<ListItemRef> {
        self.list.list.as_ref().map(|list| list.refs()).unwrap_or_default()
    }

    fn refs_of(&self, keys: &[String]) -> Vec<ListItemRef> {
        self.rows()
            .into_iter()
            .filter(|row| keys.contains(&row.key()))
            .collect()
    }

    /// The selected rows that are still in the list.
    fn chosen(&self) -> Vec<String> {
        let rows: Vec<String> = self.rows().iter().map(ListItemRef::key).collect();
        self.list
            .selected
            .iter()
            .filter(|key| rows.contains(key))
            .cloned()
            .collect()
    }

    /// A click on a row: opens it; with Shift a range is selected, with Ctrl one row more or
    /// less.
    pub fn click_row(&mut self, row: &ListItemRef, shift: bool, ctrl: bool, cx: &mut Context<Self>) {
        let key = row.key();
        let rows: Vec<String> = self.rows().iter().map(ListItemRef::key).collect();
        let from = self
            .list
            .anchor
            .as_ref()
            .and_then(|anchor| rows.iter().position(|row| row == anchor));
        cx.notify();
        if shift && let Some(from) = from {
            let to = rows.iter().position(|row| *row == key).unwrap_or(from);
            self.list.selected = rows[from.min(to)..=from.max(to)].to_vec();
            return;
        }
        self.list.anchor = Some(key.clone());
        if ctrl {
            if let Some(at) = self.list.selected.iter().position(|selected| *selected == key) {
                self.list.selected.remove(at);
            } else {
                self.list.selected.push(key);
            }
            return;
        }
        self.list.selected = vec![key];
        let window_id = self.window_id.clone();
        self.controller
            .update(cx, |controller, cx| controller.open_session(&window_id, row, None, cx));
    }

    /// A row's menu: open the selected rows (or this one), one after the other or as a group.
    pub fn row_menu(&mut self, row: &ListItemRef, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let key = row.key();
        let chosen = self.chosen();
        let keys = if chosen.contains(&key) {
            chosen
        } else {
            vec![key.clone()]
        };
        if !self.list.selected.contains(&key) {
            self.list.selected = keys.clone();
            self.list.anchor = Some(key);
        }
        let refs = self.refs_of(&keys);
        let count = refs.len();
        let open = |mode: OpenMode| MenuAction::OpenList {
            refs: refs.clone(),
            mode,
        };
        let open_label = if count > 1 {
            t(self.locale(cx), "listMenu.openMany", &[("count", &count.to_string())]).unwrap_or_default()
        } else {
            self.text("listMenu.open", cx)
        };
        let mut entries = vec![MenuEntry::new(open_label, open(OpenMode::Open))];
        if let [ListItemRef::App { session_id }] = refs.as_slice() {
            entries.push(self.entry("session.rename", MenuAction::RenameListSession(session_id.clone()), cx));
        }
        entries.push(MenuEntry {
            separator: true,
            ..self.entry("listMenu.group", open(OpenMode::Group), cx)
        });
        entries.push(self.entry("listMenu.groupInNewWindow", open(OpenMode::GroupInNewWindow), cx));
        entries
    }

    /// A workspace's menu in the list.
    fn workspace_row_menu(&self, workspace_id: &str, cx: &App) -> Vec<MenuEntry> {
        let id = || workspace_id.to_owned();
        vec![
            self.entry("workspace.open", MenuAction::OpenWorkspace(id()), cx),
            self.entry("workspace.rename", MenuAction::RenameWorkspace(id()), cx),
            MenuEntry {
                separator: true,
                ..self.entry("workspace.delete", MenuAction::DeleteWorkspace(id()), cx)
            },
        ]
    }

    /// The toolbar's Workspaces menu: save this window, update its workspace, open saved ones.
    pub fn workspace_menu(&self, cx: &App) -> Vec<MenuEntry> {
        let controller = self.controller.read(cx);
        let locale = controller.locale();
        let current = self
            .window_state(cx)
            .and_then(|window| controller.workspace_of(window))
            .map(|workspace| workspace.name.clone());
        let mut entries = vec![self.entry("workspace.save", MenuAction::SaveWorkspace, cx)];
        if let Some(name) = current {
            let label = t(locale, "workspace.update", &[("name", &name)]).unwrap_or_default();
            entries.push(MenuEntry::new(label, MenuAction::UpdateWorkspace));
        }
        let items = controller.workspace_items();
        if items.is_empty() {
            entries.push(MenuEntry {
                separator: true,
                enabled: false,
                action: None,
                ..MenuEntry::new(self.text("workspace.none", cx), MenuAction::SaveWorkspace)
            });
        }
        for (index, (id, name, open)) in items.into_iter().enumerate() {
            let label = if open {
                format!("{name} {}", self.text("workspace.openMark", cx))
            } else {
                name
            };
            entries.push(MenuEntry {
                separator: index == 0,
                ..MenuEntry::new(label, MenuAction::OpenWorkspace(id))
            });
        }
        entries
    }

    /// The button went down on a row: dragging it (or the selected rows with it) into the layout
    /// opens them there.
    fn press_row(&mut self, row: &ListItemRef, label: String, position: Point<Pixels>, cx: &mut Context<Self>) {
        let key = row.key();
        let chosen = self.chosen();
        let refs = if chosen.len() > 1 && chosen.contains(&key) {
            self.refs_of(&chosen)
        } else {
            vec![row.clone()]
        };
        // A single session of reframe's is dragged like its tile (and cannot land on itself).
        let drag_key = match refs.as_slice() {
            [ListItemRef::App { session_id }] => tile_key(session_id),
            _ => String::new(),
        };
        let label = if refs.len() > 1 {
            t(
                self.locale(cx),
                "listMenu.openMany",
                &[("count", &refs.len().to_string())],
            )
            .unwrap_or(label)
        } else {
            label
        };
        self.node_drag = Some(NodeDrag {
            key: drag_key,
            kind: DragKind::Tile,
            label,
            from: position,
            at: position,
            size: (800.0, 500.0),
            started: false,
            preview_in: None,
            list: Some(refs),
        });
        cx.notify();
    }

    /// Rows let go at a place: in a layout they open there (one of reframe's sessions shown in a
    /// window moves there like its tile); anywhere else nothing happens.
    pub fn drop_rows(
        &mut self,
        refs: Vec<ListItemRef>,
        place: Option<(String, Point<Pixels>)>,
        cx: &mut Context<Self>,
    ) -> &'static str {
        let Some((window_id, local)) = place else {
            return "nothing";
        };
        let key = match refs.as_slice() {
            [ListItemRef::App { session_id }] => tile_key(session_id),
            _ => String::new(),
        };
        // This window is being updated: it is asked directly, another one through its entity.
        let (target, area) = if window_id == self.window_id {
            self.drop_spot_at(local, &key, DragKind::Tile, cx)
        } else {
            let Some(other) = super::shell(&window_id, cx) else {
                return "nothing";
            };
            other.read(cx).drop_spot_at(local, &key, DragKind::Tile, cx)
        };
        if area.is_none() {
            return "nothing";
        }
        // One of reframe's sessions shown in a window moves there, also a collapsed one (it is
        // put back first); into an empty window it goes over as it is.
        if let [ListItemRef::App { session_id }] = refs.as_slice() {
            let from = self
                .controller
                .read(cx)
                .window_of(session_id)
                .map(|window| window.id.clone());
            if let Some(from) = from {
                return match target {
                    Some(target) => {
                        self.controller.update(cx, |controller, cx| {
                            controller.open_sessions(&window_id, refs.clone(), OpenMode::Open, Some(target), cx);
                        });
                        "moved"
                    }
                    None if from != window_id => {
                        let moved = self.controller.update(cx, |controller, cx| {
                            controller.transfer_node(&window_id, &key, &from, None, cx)
                        });
                        if moved { "transferred" } else { "nothing" }
                    }
                    None => "nothing",
                };
            }
        }
        self.controller.update(cx, |controller, cx| match refs.as_slice() {
            [single] => controller.open_session(&window_id, single, target, cx),
            _ => controller.open_sessions(&window_id, refs.clone(), OpenMode::Open, target, cx),
        });
        "opened"
    }

    // ---------- drawing ----------

    fn heading(text: String) -> Div {
        div()
            .mx(px(6.0))
            .mt(px(10.0))
            .mb(px(4.0))
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::MUTED))
            .child(text.to_uppercase())
    }

    fn empty_note(text: String) -> Div {
        div()
            .mx(px(6.0))
            .my(px(4.0))
            .text_size(px(12.0))
            .text_color(rgb(theme::MUTED))
            .child(text)
    }

    /// A row: its mark, its name with when it was last used, and a line about it.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        row: &ListItemRef,
        mark: AnyElement,
        label: String,
        used: Option<f64>,
        meta: String,
        hint: String,
        fresh: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let key = row.key();
        let selected = self.list.selected.contains(&key);
        let locale = self.locale(cx);
        let now = now_ms();
        let used = used.map(|time| {
            let exact = exact_time(time, locale, &local_time);
            (
                short_time(time, now, locale, &local_time),
                t(locale, "list.usedAt", &[("when", &exact)]).unwrap_or_default(),
            )
        });
        let tooltip = match &used {
            Some((_, exact)) => format!("{hint}\n{exact}"),
            None => hint,
        };
        let (down, click, menu) = (row.clone(), row.clone(), row.clone());
        let dragged = label.clone();
        div()
            .id(SharedString::from(format!("row-{key}")))
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .p(px(6.0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme::SURFACE_2)))
            .when(fresh, |row| row.bg(rgb(theme::ACCENT).opacity(0.28)))
            .when(selected, |row| {
                row.bg(rgb(theme::ACCENT).opacity(0.18))
                    .border_1()
                    .border_color(rgb(theme::ACCENT).opacity(0.45))
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.press_row(&down, dragged.clone(), event.position, cx);
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let modifiers = event.modifiers();
                this.click_row(&click, modifiers.shift, modifiers.control, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let entries = this.row_menu(&menu, cx);
                    this.show_menu(entries, event.position, window, cx);
                }),
            )
            .child(mark)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.0))
                            .min_w_0()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(label),
                            )
                            .when_some(used, |line, (short, _)| {
                                line.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(11.0))
                                        .text_color(rgb(theme::MUTED))
                                        .child(short),
                                )
                            }),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED))
                            .child(meta),
                    ),
            )
    }

    pub(super) fn sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let locale = self.locale(cx);
        // The language changed: the search box's placeholder with it.
        if self.list.search_locale != locale {
            let value = self.list.search.read(cx).value().to_string();
            let (search, subscription) = search_box(&value, locale, window, cx);
            self.list.search = search;
            self.list._search = subscription;
            self.list.search_locale = locale;
        }
        let text = |key: &str| t(locale, key, &[]).unwrap_or_default();
        let list = self.list.list.clone();
        let fresh = self.list.fresh.fresh().clone();
        let mut scroll = div()
            .id("sidebar-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(6.0))
            .pb(px(8.0))
            .flex()
            .flex_col();

        // Workspaces.
        let workspaces = list.as_ref().map(|list| list.workspaces.clone()).unwrap_or_default();
        if !workspaces.is_empty() {
            scroll = scroll.child(Self::heading(text("list.workspaces")));
        }
        for workspace in workspaces {
            let mark = div()
                .flex_none()
                .font_family(theme::ICON_FONT)
                .text_size(px(12.0))
                .text_color(rgb(if workspace.open { theme::ACCENT } else { theme::MUTED }))
                .child(LAYERS);
            if self.renaming() == Some(&RenameTarget::Workspace(workspace.id.clone()))
                && let Some(renaming) = &self.renaming
            {
                scroll = scroll.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .p(px(6.0))
                        .child(mark)
                        .child(self.name_field(renaming, cx)),
                );
                continue;
            }
            let when = short_time(workspace.saved_at, now_ms(), locale, &local_time);
            let meta = t(
                locale,
                "workspace.meta",
                &[("count", &workspace.sessions.to_string()), ("when", &when)],
            )
            .unwrap_or_default();
            let hint = text("list.workspaceHint");
            let (open, menu) = (workspace.id.clone(), workspace.id.clone());
            scroll = scroll.child(
                div()
                    .id(SharedString::from(format!("workspace-{}", workspace.id)))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .p(px(6.0))
                    .rounded(px(theme::RADIUS))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(theme::SURFACE_2)))
                    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.controller
                            .update(cx, |controller, cx| controller.open_workspace(&open, cx));
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            let entries = this.workspace_row_menu(&menu, cx);
                            this.show_menu(entries, event.position, window, cx);
                        }),
                    )
                    .child(mark)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(workspace.name.clone()),
                            )
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED))
                                    .child(meta),
                            ),
                    ),
            );
        }

        // reframe's own sessions.
        scroll = scroll.child(Self::heading(text("list.inApp")));
        let app = list.as_ref().map(|list| list.app.clone()).unwrap_or_default();
        if list.is_some() && app.is_empty() {
            scroll = scroll.child(Self::empty_note(text("list.emptyApp")));
        }
        for item in app {
            let row = ListItemRef::App {
                session_id: item.id.clone(),
            };
            if self.renaming() == Some(&RenameTarget::ListSession(item.id.clone()))
                && let Some(renaming) = &self.renaming
            {
                scroll = scroll.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .p(px(6.0))
                        .child(self.status_dot(item.status, cx))
                        .child(self.name_field(renaming, cx)),
                );
                continue;
            }
            let window = item
                .window_number
                .map(|n| t(locale, "list.window", &[("n", &n.to_string())]).unwrap_or_default());
            let state = if item.elsewhere {
                Some(text("list.running"))
            } else if item.window_number.is_none() {
                Some(text("list.ended"))
            } else {
                None
            };
            let meta = [Some(folder_name(&item.cwd).to_owned()), window, state]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            let hint = text(if item.window_id.is_some() {
                "list.showHint"
            } else {
                "list.openHint"
            });
            let mut entry = self.row(
                &row,
                self.status_dot(item.status, cx).into_any_element(),
                item.label.clone(),
                Some(item.last_active_at),
                meta,
                hint,
                false,
                cx,
            );
            // One in no window can be taken off the list.
            if item.window_id.is_none() {
                let id = item.id.clone();
                entry = entry.child(self.icon_button(
                    "remove",
                    icon::CLOSE,
                    text("list.remove"),
                    cx,
                    move |this, _, cx| {
                        this.controller
                            .update(cx, |controller, cx| controller.remove_session(&id, cx));
                    },
                ));
            }
            scroll = scroll.child(entry);
        }

        // Found outside reframe.
        scroll = scroll.child(Self::heading(text("list.found")));
        if let Some(list) = &list {
            for distro in &list.wsl_stopped {
                let note = t(locale, "list.wslStopped", &[("distro", distro)]).unwrap_or_default();
                scroll = scroll.child(Self::empty_note(note));
            }
            if list.running.is_empty() && list.found.is_empty() {
                scroll = scroll.child(Self::empty_note(text("list.emptyFound")));
            }
            for item in &list.running {
                let row = ListItemRef::Running {
                    pid: item.pid,
                    claude_session_id: item.claude_session_id.clone(),
                    distro: item.distro.clone(),
                };
                let mark = div()
                    .flex_none()
                    .text_size(px(9.0))
                    .text_color(rgb(theme::OK))
                    .child("▶")
                    .into_any_element();
                let label = item.name.clone().unwrap_or_else(|| folder_name(&item.cwd).to_owned());
                let meta = [
                    Some(folder_name(&item.cwd).to_owned()),
                    item.distro.clone(),
                    Some(text("list.running")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let is_new = fresh.contains(&item.claude_session_id);
                scroll = scroll.child(self.row(
                    &row,
                    mark,
                    label,
                    item.last_active_at,
                    meta,
                    text("list.openHint"),
                    is_new,
                    cx,
                ));
            }
            for item in &list.found {
                let row = ListItemRef::Found {
                    claude_session_id: item.claude_session_id.clone(),
                    distro: item.distro.clone(),
                };
                let label = item.title.clone().unwrap_or_else(|| folder_name(&item.cwd).to_owned());
                let meta = [Some(folder_name(&item.cwd).to_owned()), item.distro.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                let hint = format!("{}\n{}", item.cwd, text("list.openHint"));
                let is_new = fresh.contains(&item.claude_session_id);
                scroll = scroll.child(self.row(
                    &row,
                    self.status_dot(SessionStatus::Ended, cx).into_any_element(),
                    label,
                    Some(item.modified_at),
                    meta,
                    hint,
                    is_new,
                    cx,
                ));
            }
            if list.has_more {
                scroll = scroll.child(div().mx(px(6.0)).mt(px(8.0)).child(self.button(
                    "more",
                    text("list.more"),
                    false,
                    cx,
                    |this, _, cx| {
                        this.list.limit += LIST_PAGE;
                        this.reload_list(cx);
                    },
                )));
            }
        }

        div()
            .id("sidebar")
            .w(px(WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(theme::BORDER))
            .bg(rgb(theme::SURFACE))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && !this.list.selected.is_empty() {
                    this.list.selected.clear();
                    cx.notify();
                }
            }))
            .child(
                div()
                    .m(px(8.0))
                    .child(Input::new(&self.list.search).small().cleanable(true)),
            )
            .child(scroll)
    }
}

impl Shell {
    /// Searches the list, as typing into its search box does. For tests.
    pub fn set_search(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.list
            .search
            .update(cx, |search, cx| search.set_value(text.clone(), window, cx));
        self.list.query = text;
        self.list.limit = LIST_PAGE;
        self.reload_list(cx);
    }

    /// The list entry with that row key, if the list shows it.
    pub fn row_of(&self, key: &str) -> Option<ListItemRef> {
        self.rows().into_iter().find(|row| row.key() == key)
    }
}
