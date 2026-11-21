//! The New session, Settings and About dialogs, and the questions.
//!
//! They take the place of the Electron version's `NewSessionDialog.tsx`, `SettingsDialog.tsx`
//! and `AboutDialog.tsx`. Each is a view of its own inside the component library's dialog, which
//! brings the backdrop, Escape and the focus trap.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{Disableable as _, IndexPath, Sizable as _, Theme, ThemeMode, WindowExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use reframe_core::i18n::{Locale, resolve_locale, t};
use reframe_core::links::{AUTHOR_URL, REPO_URL};
use reframe_core::session_logic::SessionError;
use reframe_core::shortcuts::{TextSizeChange, font_size_after};
use reframe_core::state::{
    CloseWindow, FONT_SIZE_MAX, FONT_SIZE_MIN, FONT_SIZE_NORMAL, Language, MODEL_SUGGESTIONS, PERMISSION_MODES,
    PermissionMode, Preferences,
};

use crate::app::{AppEvent, Controller, NewSessionRequest};
use crate::keys;
use crate::theme;
#[cfg(windows)]
use crate::window::Shell;

type Choice = SelectState<Vec<SharedString>>;

fn words(locale: Locale) -> impl Fn(&str) -> String {
    move |key| t(locale, key, &[]).unwrap_or_else(|| key.to_owned())
}

/// The last part of a folder's path, which is what a session in it is called.
fn folder_name(folder: &str) -> String {
    folder
        .trim()
        .split(['\\', '/'])
        .rfind(|part| !part.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn field(label: String, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(div().text_color(rgb(theme::MUTED)).text_size(px(12.0)).child(label))
        .child(control)
}

fn hint(text: String) -> Div {
    div()
        .text_color(rgb(theme::MUTED))
        .text_size(px(12.0))
        .line_height(px(17.0))
        .child(text)
}

/// A dialog as 0.x drew one: in the middle of the window, its contents 18 × 20 pixels inside a
/// frame with 8 pixel corners, at 0.x's line height, and no × (Escape or a button closes it).
fn framed(dialog: Dialog) -> Dialog {
    dialog
        .centered()
        .close_button(false)
        .line_height(relative(theme::LINE_HEIGHT))
        .pt(px(18.0))
        .pb(px(18.0))
        .pl(px(20.0))
        .pr(px(20.0))
        .border_color(rgb(theme::BORDER_STRONG))
        .rounded(px(8.0))
}

/// A dialog's title, as 0.x's `h2`: 15 pixels and bold, 12 pixels above what follows.
fn heading(text: String) -> Div {
    div()
        .pb(px(4.0))
        .text_size(px(15.0))
        .font_weight(FontWeight::BOLD)
        .child(text)
}

/// A button as 0.x drew one: 30 pixels high, 13 pixel text.
fn button(id: impl Into<ElementId>, label: String) -> Button {
    Button::new(id).label(label).large().h(px(30.0)).px(px(12.0))
}

/// The dialog's main button, in the accent colour; one that cannot be used yet keeps its colour
/// at half strength, as in 0.x.
fn primary(id: impl Into<ElementId>, label: String, disabled: bool) -> Button {
    button(id, label).primary().disabled(disabled).when(disabled, |button| {
        button
            .bg(rgb(theme::ACCENT).opacity(0.5))
            .border_color(rgb(theme::ACCENT).opacity(0.0))
            .text_color(rgb(theme::ACCENT_TEXT).opacity(0.6))
    })
}

/// A checkbox with its label in 0.x's 13 pixel text.
fn check(id: impl Into<ElementId>, label: String, checked: bool) -> Checkbox {
    Checkbox::new(id).label(label).checked(checked).text_size(px(13.0))
}

/// A choice box as 0.x's `select`: a form's (32 pixels high) or a setting's (28).
fn choice_box(select: Select<Vec<SharedString>>, setting: bool) -> Select<Vec<SharedString>> {
    let (height, padding) = if setting { (28.0, 6.0) } else { (32.0, 8.0) };
    select
        .h(px(height))
        .px(px(padding))
        .text_size(px(13.0))
        .bg(rgb(theme::BG))
        .border_color(rgb(theme::BORDER_STRONG))
        .when(setting, |select| select.rounded(px(4.0)))
}

/// How wide a setting's choice box is: its longest choice, its padding and the arrow, as 0.x's
/// `select` sized itself.
fn fitted(items: &[SharedString], window: &Window) -> Pixels {
    let widest = items
        .iter()
        .map(|item| {
            let run = TextRun {
                len: item.len(),
                font: font(theme::ui_font()),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(item.clone(), px(13.0), &[run], None)
                .width
        })
        .fold(px(0.0), |widest, width| if width > widest { width } else { widest });
    widest + px(6.0 * 2.0 + 2.0 + 20.0)
}

/// A setting: what it is at the left, its control at the right.
fn setting(label: String, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(div().flex_1().min_w_0().child(label))
        .child(div().flex_none().child(control))
}

fn chip(id: impl Into<ElementId>, label: String, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_selected(selected)
        .px(px(10.0))
        .py(px(2.0))
        .rounded_full()
        .border_1()
        .border_color(rgb(if selected { theme::ACCENT } else { theme::BORDER }))
        .bg(rgb(theme::BG))
        .text_color(rgb(if selected { theme::TEXT } else { theme::MUTED }))
        .text_size(px(12.0))
        .cursor_pointer()
        .child(label)
}

fn permission_choices(default: String) -> Vec<SharedString> {
    std::iter::once(default.into())
        .chain(PERMISSION_MODES.iter().map(|mode| mode.as_str().into()))
        .collect()
}

fn permission_row(mode: Option<PermissionMode>) -> usize {
    mode.and_then(|mode| PERMISSION_MODES.iter().position(|other| *other == mode))
        .map_or(0, |at| at + 1)
}

fn permission_at(row: usize) -> Option<PermissionMode> {
    row.checked_sub(1).and_then(|at| PERMISSION_MODES.get(at).copied())
}

fn choice(items: Vec<SharedString>, row: usize, window: &mut Window, cx: &mut App) -> Entity<Choice> {
    cx.new(|cx| SelectState::new(items, Some(IndexPath::new(row)), window, cx))
}

// ---------- New session ----------

pub struct NewSessionForm {
    controller: Entity<Controller>,
    window_id: String,
    folder: Entity<InputState>,
    name: Entity<InputState>,
    worktree_name: Entity<InputState>,
    model: Entity<InputState>,
    permission: Entity<Choice>,
    /// The user typed a name: it wins over the folder's.
    custom_name: bool,
    name_from_folder: bool,
    worktree: bool,
    advanced: bool,
    recent: Vec<String>,
    /// Why the last start did not work, and the WSL distribution claude is missing in.
    error: Option<(SessionError, Option<String>)>,
    /// The folder (and a WSL distribution) is being looked at.
    busy: bool,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl NewSessionForm {
    fn new(controller: Entity<Controller>, window_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (recent, preferences, locale) = {
            let controller = controller.read(cx);
            (
                controller.recent_folders(),
                controller.state().preferences.clone(),
                controller.locale(),
            )
        };
        let text = words(locale);
        let first = recent.first().cloned().unwrap_or_default();
        let name = if preferences.name_from_folder {
            folder_name(&first)
        } else {
            String::new()
        };
        let input = |placeholder: String, value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let folder = input(text("dialog.folderPlaceholder"), first, window, cx);
        let name = input(text("dialog.namePlaceholder"), name, window, cx);
        let worktree_name = input(String::new(), String::new(), window, cx);
        // What Settings says new sessions start with.
        let model = input(
            text("dialog.modelPlaceholder"),
            preferences.default_model.clone().unwrap_or_default(),
            window,
            cx,
        );
        let permission = choice(
            permission_choices(text("dialog.default")),
            permission_row(preferences.default_permission_mode),
            window,
            cx,
        );
        let enter = |this: &mut Self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit(window, cx);
            }
        };
        let subscriptions = vec![
            cx.subscribe_in(&folder, window, move |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = None;
                    this.follow_folder(window, cx);
                    cx.notify();
                }
                enter(this, event, window, cx);
            }),
            cx.subscribe_in(&name, window, move |this, name, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.custom_name = name.read(cx).value().as_ref() != this.folder_name(cx);
                }
                enter(this, event, window, cx);
            }),
            cx.subscribe_in(
                &worktree_name,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    enter(this, event, window, cx);
                },
            ),
            cx.subscribe_in(&model, window, move |this, _, event: &InputEvent, window, cx| {
                enter(this, event, window, cx);
            }),
        ];
        Self {
            controller,
            window_id,
            folder,
            name,
            worktree_name,
            model,
            permission,
            custom_name: false,
            name_from_folder: preferences.name_from_folder,
            worktree: false,
            advanced: false,
            recent,
            error: None,
            busy: false,
            pending: None,
            _subscriptions: subscriptions,
        }
    }

    /// The name the folder gives the session, if it is to.
    fn folder_name(&self, cx: &App) -> String {
        if self.name_from_folder {
            folder_name(&self.folder.read(cx).value())
        } else {
            String::new()
        }
    }

    /// The name goes with the folder until one is typed.
    fn follow_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.custom_name {
            let name = self.folder_name(cx);
            self.name.update(cx, |input, cx| input.set_value(name, window, cx));
        }
    }

    fn set_folder(&mut self, folder: String, window: &mut Window, cx: &mut Context<Self>) {
        self.folder.update(cx, |input, cx| input.set_value(folder, window, cx));
        self.error = None;
        self.follow_folder(window, cx);
        cx.notify();
    }

    fn set_name_from_folder(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.name_from_folder = on;
        self.controller.update(cx, |controller, cx| {
            controller.update_preferences(cx, |preferences| preferences.name_from_folder = on);
        });
        // Back on: follow the folder again. Off: drop the folder name, but keep anything typed.
        if on {
            self.custom_name = false;
        }
        self.follow_folder(window, cx);
        cx.notify();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(folder) = paths.first()
            {
                let folder = folder.to_string_lossy().into_owned();
                let _ = this.update_in(cx, |this, window, cx| this.set_folder(folder, window, cx));
            }
        })
        .detach();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let cwd = value(&self.folder);
        if cwd.is_empty() || self.busy {
            return;
        }
        let some = |text: String| Some(text).filter(|text| !text.is_empty());
        let request = NewSessionRequest {
            cwd,
            name: some(value(&self.name)),
            model: some(value(&self.model)),
            permission_mode: self
                .permission
                .read(cx)
                .selected_index(cx)
                .and_then(|index| permission_at(index.row)),
            worktree: self.worktree,
            worktree_name: some(value(&self.worktree_name)).filter(|_| self.worktree),
            target: None,
        };
        let window_id = self.window_id.clone();
        let created = self.controller.update(cx, |controller, cx| {
            controller.create_session_checked(&window_id, request, cx)
        });
        // The session is made also if the dialog goes meanwhile: the dialog only shows how it went.
        let (outcome, outcome_received) = futures::channel::oneshot::channel();
        cx.spawn(async move |_, _| {
            let _ = outcome.send(created.await);
        })
        .detach();
        self.busy = true;
        self.error = None;
        cx.notify();
        let handle = window.window_handle();
        self.pending = Some(cx.spawn(async move |this, cx| {
            let Ok(created) = outcome_received.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match created {
                    Ok(_) => {
                        let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
    }
}

impl Render for NewSessionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = words(self.controller.read(cx).locale());
        let folder = self.folder.read(cx).value();
        let mut form = div().flex().flex_col().gap(px(12.0)).child(field(
            text("dialog.folder"),
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(theme::field(Input::new(&self.folder)).aria_label(text("dialog.folder"))),
                )
                .child(
                    button("browse", text("dialog.browse"))
                        .h(px(32.0))
                        .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                ),
        ));
        if !self.recent.is_empty() {
            let mut recent = div()
                .id("recent")
                .role(Role::Group)
                .aria_label(text("dialog.recent"))
                .mt(px(-4.0))
                .flex()
                .flex_wrap()
                .gap(px(6.0));
            for (index, path) in self.recent.iter().enumerate() {
                let name = folder_name(path);
                let label = if name.is_empty() { path.clone() } else { name };
                let path = path.clone();
                recent = recent.child(
                    chip(("recent", index), label, path.as_str() == folder.as_ref()).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.set_folder(path.clone(), window, cx);
                        },
                    )),
                );
            }
            form = form.child(recent);
        }
        form = form
            .child(field(
                text("dialog.name"),
                theme::field(Input::new(&self.name)).aria_label(text("dialog.name")),
            ))
            .child(
                check("name-from-folder", text("dialog.nameFromFolder"), self.name_from_folder)
                    .mt(px(-6.0))
                    .text_size(px(12.0))
                    .text_color(rgb(theme::MUTED))
                    .on_click(cx.listener(|this, on: &bool, window, cx| {
                        this.set_name_from_folder(*on, window, cx);
                    })),
            )
            .child(
                check("worktree", text("dialog.worktree"), self.worktree).on_click(cx.listener(
                    |this, on: &bool, _, cx| {
                        this.worktree = *on;
                        cx.notify();
                    },
                )),
            );
        if self.worktree {
            form = form.child(div().pl(px(24.0)).child(field(
                text("dialog.worktreeName"),
                theme::field(Input::new(&self.worktree_name)).aria_label(text("dialog.worktreeName")),
            )));
        }
        form = form.child(
            div()
                .id("advanced")
                .role(Role::Button)
                .aria_label(text("dialog.advanced"))
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(12.0))
                .text_color(rgb(theme::MUTED))
                .cursor_pointer()
                .hover(|style| style.text_color(rgb(theme::TEXT)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.advanced = !this.advanced;
                    cx.notify();
                }))
                .child(if self.advanced { "▾" } else { "▸" })
                .child(text("dialog.advanced")),
        );
        if self.advanced {
            let model = self.model.read(cx).value();
            let mut suggestions = div().mt(px(-4.0)).flex().flex_wrap().gap(px(6.0));
            for (index, suggestion) in MODEL_SUGGESTIONS.into_iter().enumerate() {
                suggestions = suggestions.child(
                    chip(("model", index), suggestion.to_owned(), model.as_ref() == suggestion).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.model
                                .update(cx, |input, cx| input.set_value(suggestion, window, cx));
                            cx.notify();
                        },
                    )),
                );
            }
            form = form
                .child(field(
                    text("dialog.model"),
                    theme::field(Input::new(&self.model)).aria_label(text("dialog.model")),
                ))
                .child(suggestions)
                .child(field(
                    text("dialog.permissionMode"),
                    choice_box(
                        Select::new(&self.permission).accessibility_label(text("dialog.permissionMode")),
                        false,
                    ),
                ));
        }
        if let Some((error, distro)) = &self.error {
            let key = format!("dialog.error.{}", error.key());
            let locale = self.controller.read(cx).locale();
            let message = match distro {
                Some(distro) => t(locale, "error.claudeNotFoundInWsl", &[("distro", distro)]).unwrap_or_default(),
                None => {
                    let message = t(locale, &key, &[]).unwrap_or_else(|| text(&format!("tile.error.{}", error.key())));
                    if *error == SessionError::ClaudeNotFound {
                        format!("{message} {}", text("error.claudeOnlyInWsl"))
                    } else {
                        message
                    }
                }
            };
            form = form.child(
                div()
                    .id("error")
                    .role(Role::Label)
                    .aria_value(message.clone())
                    .text_color(rgb(theme::DANGER))
                    .child(message),
            );
        }
        form.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(button("cancel", text("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                .child(
                    primary("start", text("dialog.start"), folder.trim().is_empty() || self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                ),
        )
    }
}

pub fn open_new_session(controller: &Entity<Controller>, window_id: &str, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| NewSessionForm::new(controller.clone(), window_id.to_owned(), window, cx));
    let controller = controller.clone();
    let folder = form.read(cx).folder.focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let title = words(controller.read(cx).locale())("dialog.title");
        framed(dialog)
            .title(heading(title))
            .w(px(520.0))
            .child(form.clone())
            // Enter starts the session (the form's fields take it); the dialog closes once it runs.
            .on_ok(|_, _, _| false)
    });
    // The folder field has the keyboard once the dialog is drawn. Asked for while the dialog
    // draws itself, it could go unseen until some other frame came (on CI none did).
    window.on_next_frame(move |window, cx| folder.focus(window, cx));
}

// ---------- Settings ----------

/// Settings: the language, the terminals' text size, snapping separators to a grid, what closing
/// a window with sessions does, which claude runs and what new sessions start with. Each applies
/// at once. Starting with the system and the update checks follow with the tray and the updater.
pub struct SettingsForm {
    controller: Entity<Controller>,
    language: Entity<Choice>,
    close_window: Entity<Choice>,
    permission: Entity<Choice>,
    model: Entity<InputState>,
    /// The language the choices are written in.
    locale: Locale,
    _choices: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

const LANGUAGES: [Language; 3] = [Language::System, Language::En, Language::De];
const CLOSE_WINDOW: [CloseWindow; 3] = [CloseWindow::Ask, CloseWindow::End, CloseWindow::Keep];

/// What "System" stands for right now, named in the current language.
fn system_language_name(system_locale: &str, locale: Locale) -> String {
    let german = locale == Locale::De;
    let tag = system_locale.to_lowercase();
    let name = if tag.starts_with("de") {
        if german { "Deutsch" } else { "German" }
    } else if tag.starts_with("en") {
        if german { "Englisch" } else { "English" }
    } else {
        // Other languages show in English, and by their tag.
        return format!(
            "{}, {system_locale}",
            if resolve_locale(system_locale) == Locale::De {
                "Deutsch"
            } else {
                "English"
            }
        );
    };
    name.to_owned()
}

type Choices = (Entity<Choice>, Entity<Choice>, Entity<Choice>, Vec<Subscription>);

impl SettingsForm {
    fn new(controller: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (locale, preferences) = {
            let controller = controller.read(cx);
            (controller.locale(), controller.state().preferences.clone())
        };
        let text = words(locale);
        let model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(text("dialog.modelPlaceholder"))
                .default_value(preferences.default_model.clone().unwrap_or_default())
        });
        let subscriptions = vec![
            // Typed here, kept when the field is left.
            cx.subscribe_in(&model, window, |this, model, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let typed: String = model.read(cx).value().trim().chars().take(100).collect();
                    this.set(cx, move |preferences| {
                        preferences.default_model = Some(typed).filter(|model| !model.is_empty());
                    });
                }
            }),
            cx.subscribe_in(&controller, window, |this, controller, event: &AppEvent, window, cx| {
                if matches!(event, AppEvent::Preferences) {
                    // Another language: the choices are written anew.
                    if controller.read(cx).locale() != this.locale {
                        let (language, close_window, permission, choices) = Self::choices(&this.controller, window, cx);
                        this.language = language;
                        this.close_window = close_window;
                        this.permission = permission;
                        this._choices = choices;
                        this.locale = controller.read(cx).locale();
                    }
                    cx.notify();
                }
            }),
        ];
        let (language, close_window, permission, choices) = Self::choices(&controller, window, cx);
        Self {
            controller,
            language,
            close_window,
            permission,
            model,
            locale,
            _choices: choices,
            _subscriptions: subscriptions,
        }
    }

    /// What the three lists offer, in the current language: languages, what closing a window
    /// does, permission modes.
    fn lists(controller: &Entity<Controller>, cx: &App) -> [Vec<SharedString>; 3] {
        let (locale, system) = {
            let controller = controller.read(cx);
            (controller.locale(), controller.config().system_locale.clone())
        };
        let text = words(locale);
        let system_name = system_language_name(&system, locale);
        let languages: Vec<SharedString> = vec![
            t(locale, "settings.languageSystem", &[("language", &system_name)])
                .unwrap_or_default()
                .into(),
            "English".into(),
            "Deutsch".into(),
        ];
        let closing: Vec<SharedString> = [
            "settings.closeWindowAsk",
            "settings.closeWindowEnd",
            "settings.closeWindowKeep",
        ]
        .iter()
        .map(|key| text(key).into())
        .collect();
        [languages, closing, permission_choices(text("dialog.default"))]
    }

    /// The three lists to choose from, in the current language, showing what is set.
    fn choices(controller: &Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Choices {
        let preferences = controller.read(cx).state().preferences.clone();
        let [languages, closing, permissions] = Self::lists(controller, cx);
        let row = |found: Option<usize>| found.unwrap_or(0);
        let language = choice(
            languages,
            row(LANGUAGES.iter().position(|language| *language == preferences.language)),
            window,
            cx,
        );
        let close_window = choice(
            closing,
            row(CLOSE_WINDOW.iter().position(|close| *close == preferences.close_window)),
            window,
            cx,
        );
        let permission = choice(
            permissions,
            permission_row(preferences.default_permission_mode),
            window,
            cx,
        );
        let chosen =
            |select: &Entity<Choice>, cx: &App| select.read(cx).selected_index(cx).map_or(0, |index| index.row);
        let subscriptions = vec![
            cx.subscribe(
                &language,
                move |this, select, _: &SelectEvent<Vec<SharedString>>, cx| {
                    let language = LANGUAGES[chosen(&select, cx).min(LANGUAGES.len() - 1)];
                    this.set(cx, move |preferences| preferences.language = language);
                },
            ),
            cx.subscribe(
                &close_window,
                move |this, select, _: &SelectEvent<Vec<SharedString>>, cx| {
                    let close = CLOSE_WINDOW[chosen(&select, cx).min(CLOSE_WINDOW.len() - 1)];
                    this.set(cx, move |preferences| preferences.close_window = close);
                },
            ),
            cx.subscribe(
                &permission,
                move |this, select, _: &SelectEvent<Vec<SharedString>>, cx| {
                    let mode = permission_at(chosen(&select, cx));
                    this.set(cx, move |preferences| preferences.default_permission_mode = mode);
                },
            ),
        ];
        (language, close_window, permission, subscriptions)
    }

    fn set(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Preferences)) {
        self.controller
            .update(cx, |controller, cx| controller.update_preferences(cx, change));
    }

    fn text_size(&mut self, change: TextSizeChange, cx: &mut Context<Self>) {
        self.controller
            .update(cx, |controller, cx| controller.change_text_size(change, cx));
    }

    fn choose_claude(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file) = cx.try_global::<NextChosenFile>().and_then(|next| next.0.clone()) {
            cx.set_global(NextChosenFile(None));
            self.set(cx, move |preferences| preferences.claude_bin = Some(file));
            return;
        }
        // Linux has no file types for programs: any file can be one.
        #[cfg(windows)]
        {
            let locale = self.controller.read(cx).locale();
            let text = |key: &str| t(locale, key, &[]).unwrap_or_default();
            gpui_windows::set_next_open_file_types(vec![
                (text("settings.programs"), "*.exe;*.cmd;*.bat".to_owned()),
                (text("settings.allFiles"), "*.*".to_owned()),
            ]);
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(file) = paths.first()
            {
                let file: String = file.to_string_lossy().chars().take(1024).collect();
                let _ = this.update(cx, |this, cx| {
                    this.set(cx, move |preferences| preferences.claude_bin = Some(file));
                });
            }
        })
        .detach();
    }
}

impl Render for SettingsForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (locale, preferences, claude) = {
            let controller = self.controller.read(cx);
            (
                controller.locale(),
                controller.state().preferences.clone(),
                controller.claude_source(),
            )
        };
        let text = words(locale);
        let size = preferences.font_size;
        let step =
            |id: &'static str, label: String, hint: Option<String>, change: TextSizeChange, cx: &mut Context<Self>| {
                button(id, label)
                    .min_w(px(30.0))
                    .when_some(hint, |button, hint| button.tooltip(hint))
                    .disabled(font_size_after(size, change) == size)
                    .on_click(cx.listener(move |this, _, _, cx| this.text_size(change, cx)))
            };
        let text_size = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(step(
                "smaller",
                "−".to_owned(),
                Some(text("settings.smaller")),
                TextSizeChange::Smaller,
                cx,
            ))
            .child(div().min_w(px(16.0)).text_center().child(size.to_string()))
            .child(step(
                "bigger",
                "+".to_owned(),
                Some(text("settings.bigger")),
                TextSizeChange::Bigger,
                cx,
            ))
            .child(step("normal", text("settings.normal"), None, TextSizeChange::Reset, cx));
        let size_hint = t(
            locale,
            "settings.textSizeHint",
            &[
                ("bigger", &keys::label(locale, "=", false)),
                ("smaller", &keys::label(locale, "-", false)),
                ("normal", &keys::label(locale, "0", false)),
                ("ctrl", &text("keys.ctrl")),
            ],
        )
        .unwrap_or_default();
        debug_assert!((FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&FONT_SIZE_NORMAL));

        let chosen = preferences.claude_bin.clone();
        let claude_buttons = div()
            .flex()
            .gap(px(6.0))
            .when(chosen.is_some(), |row| {
                row.child(
                    button("claude-path", text("settings.claudeUsePath")).on_click(cx.listener(|this, _, _, cx| {
                        this.set(cx, |preferences| preferences.claude_bin = None);
                    })),
                )
            })
            .child(
                button("claude-choose", text("settings.claudeChoose"))
                    .on_click(cx.listener(|this, _, window, cx| this.choose_claude(window, cx))),
            );
        let claude_state = text(match (&chosen, &claude) {
            (Some(_), Some(_)) => "settings.claudeChosen",
            (Some(_), None) => "settings.claudeMissing",
            (None, Some(_)) => "settings.claudeFound",
            (None, None) => "settings.claudeNotFound",
        });
        let claude_path = chosen.or(claude).unwrap_or_default();
        // Each list's box as wide as its longest choice, as 0.x's were.
        let [languages, closing, permissions] = Self::lists(&self.controller, cx);
        let setting_choice = |state: &Entity<Choice>, label: String, items: &[SharedString]| {
            div()
                .w(fitted(items, window))
                .child(choice_box(Select::new(state).accessibility_label(label), true))
        };
        // A setting's explanation sits closer to it than the next setting does.
        let below = |line: Div| line.mt(px(-6.0));

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(setting(
                text("settings.language"),
                setting_choice(&self.language, text("settings.language"), &languages),
            ))
            .child(setting(text("settings.textSize"), text_size))
            .child(below(hint(size_hint)))
            .child(
                check("snap", text("settings.snap"), preferences.snap_to_grid).on_click(cx.listener(
                    |this, on: &bool, _, cx| {
                        let on = *on;
                        this.set(cx, move |preferences| preferences.snap_to_grid = on);
                    },
                )),
            )
            .child(below(hint(text("settings.snapHint"))))
            .child(setting(
                text("settings.closeWindow"),
                setting_choice(&self.close_window, text("settings.closeWindow"), &closing),
            ))
            .child(below(hint(text("settings.closeWindowHint"))))
            .child(
                check(
                    "autostart",
                    text("settings.autostart"),
                    self.controller.read(cx).starts_with_system(),
                )
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    let on = *on;
                    this.controller.update(cx, |controller, cx| {
                        controller.set_start_with_system(on, cx);
                    });
                    cx.notify();
                })),
            )
            .child(below(hint(text("settings.autostartHint"))))
            .child(setting(text("settings.claude"), claude_buttons))
            .child({
                let line = format!("{claude_state} {claude_path}");
                div()
                    .id("claude")
                    .role(Role::Label)
                    .aria_value(line)
                    .mt(px(-6.0))
                    .flex()
                    .gap(px(6.0))
                    .text_color(rgb(theme::MUTED))
                    .text_size(px(12.0))
                    .line_height(px(17.0))
                    .child(div().flex_none().child(claude_state))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .child(claude_path),
                    )
            })
            .child(below(hint(text("settings.claudeHint"))))
            .child(setting(
                text("settings.defaultModel"),
                div().w(px(160.0)).child(
                    Input::new(&self.model)
                        .aria_label(text("settings.defaultModel"))
                        .h(px(28.0))
                        .px(px(8.0))
                        .text_size(px(13.0))
                        .bg(rgb(theme::TERMINAL))
                        .border_color(rgb(theme::BORDER))
                        .rounded(px(4.0)),
                ),
            ))
            .child(setting(
                text("settings.defaultPermissionMode"),
                setting_choice(&self.permission, text("settings.defaultPermissionMode"), &permissions),
            ))
            .child(below(hint(text("settings.launchHint"))))
            // Only where this copy checks at all.
            .when(self.controller.read(cx).update_checks(), |form| {
                form.child(
                    check("updates", text("settings.updates"), preferences.check_for_updates).on_click(cx.listener(
                        |this, on: &bool, _, cx| {
                            let on = *on;
                            this.set(cx, move |preferences| preferences.check_for_updates = on);
                        },
                    )),
                )
                .child(below(hint(text("settings.updatesHint"))))
            })
            .child(div().flex().justify_end().child(
                primary("close", text("settings.close"), false).on_click(|_, window, cx| window.close_dialog(cx)),
            ))
    }
}

/// Test hook (REFRAME_E2E): the file the next "Choose…" picks instead of asking the user.
#[derive(Default)]
pub struct NextChosenFile(pub Option<String>);

impl Global for NextChosenFile {}

pub fn open_settings(controller: &Entity<Controller>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| SettingsForm::new(controller.clone(), window, cx));
    let controller = controller.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let dialog = framed(dialog);
        let title = words(controller.read(cx).locale())("settings.title");
        // Enter in a field keeps Settings open (0.x's were no form); its button closes it.
        dialog
            .title(heading(title))
            .w(px(460.0))
            .child(form.clone())
            .on_ok(|_, _, _| false)
    });
}

// ---------- questions (the system's message boxes, as in 0.x) ----------

/// Asks as 0.x's message boxes did (Electron's `dialog.showMessageBox`): the system's task dialog,
/// titled `title`, with the answers other than OK and Cancel as command links, `default` chosen
/// with Enter and `cancel` given by Escape. Electron told OK and Cancel apart by their English
/// label, so in German every answer is a command link, as it was. The answer comes as its index.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn message_box(
    level: PromptLevel,
    title: &str,
    message: &str,
    detail: Option<&str>,
    answers: &[String],
    (default, cancel): (usize, usize),
    checkbox: Option<(String, Arc<AtomicBool>)>,
    window: &mut Window,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<usize> {
    let buttons: Vec<PromptButton> = answers
        .iter()
        .map(|label| match label.to_lowercase().as_str() {
            "ok" => PromptButton::Ok(label.clone().into()),
            "cancel" => PromptButton::Cancel(label.clone().into()),
            _ => PromptButton::Other(label.clone().into()),
        })
        .collect();
    gpui_windows::set_next_prompt(gpui_windows::PromptStyle {
        title: Some(title.to_owned()),
        command_links: true,
        checkbox,
        default: Some(default),
        cancel: Some(cancel),
        as_electron: true,
    });
    window.prompt(level, message, detail, &buttons, cx)
}

/// Asks in the window, as a dialog of reframe's own (GPUI has none of the system's on Linux):
/// titled `title`, the answers as buttons in their order, `default` taken by Enter and `cancel`
/// by Escape. The answer comes as its index; none when the dialog went another way.
#[cfg(not(windows))]
#[allow(clippy::too_many_arguments)]
fn message_box(
    _level: PromptLevel,
    title: &str,
    message: &str,
    detail: Option<&str>,
    answers: &[String],
    (default, cancel): (usize, usize),
    checkbox: Option<(String, Arc<AtomicBool>)>,
    window: &mut Window,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<usize> {
    let (sender, receiver) = futures::channel::oneshot::channel();
    let answer = Answer(std::rc::Rc::new(std::cell::RefCell::new(Some(sender))));
    let form = cx.new(|_| MessageForm {
        message: message.to_owned(),
        detail: detail.map(str::to_owned),
        answers: answers.to_vec(),
        default,
        checkbox,
        checked: false,
        answer: answer.clone(),
    });
    let title = title.to_owned();
    window.open_dialog(cx, move |dialog, _, _| {
        let (on_enter, on_escape) = (answer.clone(), answer.clone());
        framed(dialog)
            .title(heading(title.clone()))
            .w(px(460.0))
            .overlay_closable(false)
            .child(form.clone())
            .on_ok(move |_, _, _| on_enter.give(default))
            .on_cancel(move |_, _, _| on_escape.give(cancel))
    });
    receiver
}

/// Where a question's answer goes, once.
#[cfg(not(windows))]
#[derive(Clone)]
struct Answer(std::rc::Rc<std::cell::RefCell<Option<futures::channel::oneshot::Sender<usize>>>>);

#[cfg(not(windows))]
impl Answer {
    /// Sends the answer (the first one counts). True: the dialog may close.
    fn give(&self, index: usize) -> bool {
        if let Some(sender) = self.0.borrow_mut().take() {
            let _ = sender.send(index);
        }
        true
    }
}

/// The inside of a question: the message, its detail, a checkbox, the answers.
#[cfg(not(windows))]
struct MessageForm {
    message: String,
    detail: Option<String>,
    answers: Vec<String>,
    default: usize,
    checkbox: Option<(String, Arc<AtomicBool>)>,
    checked: bool,
    answer: Answer,
}

#[cfg(not(windows))]
impl Render for MessageForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.answers.iter().enumerate().map(|(index, label)| {
            let button = if index == self.default {
                primary(("answer", index), label.clone(), false)
            } else {
                button(("answer", index), label.clone())
            };
            button.on_click(cx.listener(move |this, _, window, cx| {
                this.answer.give(index);
                window.close_dialog(cx);
            }))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(div().child(self.message.clone()))
            .when_some(self.detail.clone(), |form, detail| {
                form.child(div().text_color(rgb(theme::MUTED)).child(detail))
            })
            .when_some(self.checkbox.as_ref().map(|(label, _)| label.clone()), |form, label| {
                form.child(
                    check("remember", label, self.checked).on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.checked = *on;
                        if let Some((_, remember)) = &this.checkbox {
                            remember.store(*on, std::sync::atomic::Ordering::Release);
                        }
                        cx.notify();
                    })),
                )
            })
            .child(div().mt(px(8.0)).flex().justify_end().gap(px(8.0)).children(buttons))
    }
}

/// "End the sessions in this window?" (0.x's `askToClose`): end them, keep them running in the
/// tray, or cancel; and whether to ask again. The answer: 0 end, 1 keep, 2 cancel.
pub fn ask_to_close(
    controller: &Entity<Controller>,
    count: usize,
    remember: Arc<AtomicBool>,
    window: &mut Window,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<usize> {
    let locale = controller.read(cx).locale();
    let text = words(locale);
    let message = if count == 1 {
        text("closeWindow.messageOne")
    } else {
        t(locale, "closeWindow.message", &[("count", &count.to_string())]).unwrap_or_default()
    };
    // Without a tray to keep it in (some Linux desktops), a kept window waits minimized.
    #[cfg(windows)]
    let (keep, detail) = ("closeWindow.keep", "closeWindow.detail");
    #[cfg(not(windows))]
    let (keep, detail) = if crate::platform::has_tray() {
        ("closeWindow.keep", "closeWindow.detail")
    } else {
        ("closeWindow.keepNoTray", "closeWindow.detailNoTray")
    };
    message_box(
        PromptLevel::Info,
        &text("closeWindow.title"),
        &message,
        Some(&text(detail)),
        &[text("closeWindow.end"), text(keep), text("dialog.cancel")],
        (1, 2),
        Some((text("closeWindow.remember"), remember)),
        window,
        cx,
    )
}

/// A question with one or two answers, or a message with OK. Yes is answer 0. Something that
/// cannot be undone is not the default (0.x's `defaultId: 1` for deleting a workspace).
pub fn ask(
    question: &crate::window::Question,
    window: &mut Window,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<usize> {
    let (answers, default) = match &question.cancel {
        Some(cancel) => (
            vec![question.confirm.clone(), cancel.clone()],
            usize::from(question.danger),
        ),
        // A message (0.x gave it no buttons, which is the system's OK).
        None => (vec!["OK".to_owned()], 0),
    };
    let level = if question.error {
        PromptLevel::Critical
    } else {
        PromptLevel::Info
    };
    let cancel = answers.len() - 1;
    message_box(
        level,
        &question.title,
        &question.message,
        question.detail.as_deref(),
        &answers,
        (default, cancel),
        None,
        window,
        cx,
    )
}

// ---------- taking over from a terminal window ----------

/// Confirms taking over the session(s) of a terminal window that was dropped onto this window;
/// with several, which ones.
#[cfg(windows)]
pub struct TerminalTakeoverForm {
    shell: WeakEntity<Shell>,
    controller: Entity<Controller>,
    candidates: Vec<crate::window::TerminalCandidate>,
    picked: Vec<String>,
}

#[cfg(windows)]
impl TerminalTakeoverForm {
    fn answer(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = confirmed.then(|| self.picked.clone());
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.answer_terminal(chosen, window, cx));
    }
}

#[cfg(windows)]
impl Render for TerminalTakeoverForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = words(self.controller.read(cx).locale());
        let single = self.candidates.len() == 1;
        let mut list = div().flex().flex_col().gap(px(4.0));
        for candidate in &self.candidates {
            let id = candidate.claude_session_id.clone();
            let label = candidate.name.clone().unwrap_or_else(|| folder_name(&candidate.cwd));
            let entry = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::MUTED))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(candidate.cwd.clone()),
                );
            // Each one in a box of its own, as 0.x listed them.
            let item = div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(8.0))
                .py(px(6.0))
                .border_1()
                .border_color(rgb(theme::BORDER))
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::BG));
            list = list.child(if single {
                item.child(entry)
            } else {
                item.child(
                    Checkbox::new(SharedString::from(format!("take-{id}")))
                        .checked(self.picked.contains(&id))
                        .on_click(cx.listener(move |this, on: &bool, _, cx| {
                            this.picked.retain(|picked| *picked != id);
                            if *on {
                                this.picked.push(id.clone());
                            }
                            cx.notify();
                        })),
                )
                .child(entry)
            });
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(div().child(text(if single { "foreign.single" } else { "foreign.multiple" })))
            .child(list)
            .child(hint(text("foreign.detail")))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        button("cancel", text("dialog.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.answer(false, window, cx))),
                    )
                    .child(
                        primary("confirm", text("takeover.confirm"), self.picked.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| this.answer(true, window, cx))),
                    ),
            )
    }
}

#[cfg(windows)]
pub fn open_terminal_takeover(
    shell: WeakEntity<Shell>,
    controller: &Entity<Controller>,
    candidates: Vec<crate::window::TerminalCandidate>,
    window: &mut Window,
    cx: &mut App,
) {
    let picked = candidates
        .iter()
        .filter(|candidate| candidate.preselected)
        .map(|candidate| candidate.claude_session_id.clone())
        .collect();
    let form = cx.new(|_| TerminalTakeoverForm {
        shell,
        controller: controller.clone(),
        candidates,
        picked,
    });
    let controller = controller.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let dialog = framed(dialog);
        let title = words(controller.read(cx).locale())("foreign.title");
        dialog.title(heading(title)).w(px(520.0)).child(form.clone())
    });
}

// ---------- saving a workspace ----------

/// Names this window before it is saved as a workspace.
pub struct SaveWorkspaceForm {
    controller: Entity<Controller>,
    window_id: String,
    name: Entity<InputState>,
    suggestion: String,
    _subscriptions: Vec<Subscription>,
}

impl SaveWorkspaceForm {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.name.read(cx).value().trim().to_owned();
        let name = if typed.is_empty() {
            self.suggestion.clone()
        } else {
            typed
        };
        let window_id = self.window_id.clone();
        self.controller
            .update(cx, |controller, cx| controller.save_workspace(&window_id, &name, cx));
        window.close_dialog(cx);
    }
}

impl Render for SaveWorkspaceForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = words(self.controller.read(cx).locale());
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(hint(text("workspace.nameHint")))
            .child(field(
                text("workspace.nameLabel"),
                theme::field(Input::new(&self.name)).aria_label(text("workspace.nameLabel")),
            ))
            .child(
                div()
                    .flex()
                    .justify_center()
                    .gap(px(8.0))
                    .child(button("cancel", text("dialog.cancel")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(
                        primary("save", text("workspace.saveButton"), false)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

pub fn open_save_workspace(controller: &Entity<Controller>, window_id: &str, window: &mut Window, cx: &mut App) {
    let (locale, count) = {
        let controller = controller.read(cx);
        (controller.locale(), controller.state().workspaces.len())
    };
    let suggestion = t(locale, "workspace.defaultName", &[("n", &(count + 1).to_string())]).unwrap_or_default();
    let form = cx.new(|cx| {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(suggestion.clone()));
        let entered = cx.subscribe_in(
            &name,
            window,
            |this: &mut SaveWorkspaceForm, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save(window, cx);
                }
            },
        );
        SaveWorkspaceForm {
            controller: controller.clone(),
            window_id: window_id.to_owned(),
            name,
            suggestion,
            _subscriptions: vec![entered],
        }
    });
    let name = form.read(cx).name.clone();
    window.on_next_frame(move |window, cx| {
        // As the folder of a new session: once the dialog is drawn.
        name.update(cx, |name, cx| {
            name.focus(window, cx);
            name.select_all(window, cx);
        });
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        let title = words(form.read(cx).controller.read(cx).locale())("workspace.nameTitle");
        // Enter saves (the name field takes it), and the dialog closes then.
        framed(dialog)
            .title(heading(title))
            .w(px(520.0))
            .child(form.clone())
            .on_ok(|_, _, _| false)
    });
}

// ---------- About ----------

/// About reframe: version, where it comes from, and its license.
pub struct AboutForm {
    controller: Entity<Controller>,
    logo: Arc<Image>,
    license_open: bool,
}

/// The license's paragraphs, without the file's line breaks inside them (the dialog wraps them
/// itself).
fn license_paragraphs() -> Vec<String> {
    include_str!("../../../LICENSE")
        .replace("\r\n", "\n")
        .trim()
        .split("\n\n")
        .map(|paragraph| paragraph.replace('\n', " "))
        .collect()
}

impl Render for AboutForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.controller.read(cx).locale();
        let text = words(locale);
        let link = |id: &'static str, label: String, url: &'static str| {
            div()
                .id(id)
                .role(Role::Link)
                .aria_label(label.clone())
                .text_color(rgb(theme::ACCENT))
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(move |_, _, cx| crate::browser::open(url, cx))
                .child(label)
        };
        let version = t(locale, "about.version", &[("version", env!("CARGO_PKG_VERSION"))]).unwrap_or_default();
        // 0.x's `.modal.about`: centred, 4 pixels between lines, more around the links.
        let mut about = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .text_center()
            .child(img(self.logo.clone()).size(px(72.0)).mt(px(4.0)))
            .child(
                div()
                    .mt(px(6.0))
                    .text_size(px(20.0))
                    .font_weight(FontWeight::BOLD)
                    .child("reframe"),
            )
            .child(
                div()
                    .id("version")
                    .role(Role::Label)
                    .aria_value(version.clone())
                    .my(px(2.0))
                    .text_color(rgb(theme::MUTED))
                    .child(version),
            )
            .child(
                div()
                    .my(px(2.0))
                    .text_color(rgb(theme::MUTED))
                    .child(text("about.tagline")),
            )
            .child(
                div()
                    .my(px(8.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(link("github", text("about.github"), REPO_URL))
                    .child(link("star", format!("⭐ {}", text("about.star")), REPO_URL)),
            )
            .child(
                div()
                    .my(px(8.0))
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .id("made-with")
                            .role(Role::Label)
                            .aria_value(text("about.madeWith"))
                            .child(text("about.madeWith")),
                    )
                    .child(link("author", "TheyCallMeJuzou".to_owned(), AUTHOR_URL)),
            )
            .child(
                div()
                    .id("license")
                    .role(Role::Button)
                    .aria_label(text("about.license"))
                    .w_full()
                    .mt(px(6.0))
                    .flex()
                    .gap(px(6.0))
                    .text_color(rgb(theme::MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(theme::TEXT)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.license_open = !this.license_open;
                        cx.notify();
                    }))
                    .child(if self.license_open { "▾" } else { "▸" })
                    .child(text("about.license")),
            );
        if self.license_open {
            let mut license = div()
                .id("license-text")
                .role(Role::Document)
                .aria_label(license_paragraphs().join("\n\n"))
                .w_full()
                .max_h(px(180.0))
                .overflow_y_scroll()
                .p(px(8.0))
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::BG))
                .text_size(px(11.0))
                .text_left()
                .text_color(rgb(theme::MUTED))
                .flex()
                .flex_col()
                .gap(px(8.0));
            for paragraph in license_paragraphs() {
                license = license.child(div().child(paragraph));
            }
            about = about.child(license);
        }
        about.child(
            div()
                .mt(px(4.0))
                .flex()
                .justify_center()
                .child(primary("close", text("about.close"), false).on_click(|_, window, cx| window.close_dialog(cx))),
        )
    }
}

pub fn open_about(controller: &Entity<Controller>, window: &mut Window, cx: &mut App) {
    let logo = Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/reframe-icon-256.png").to_vec(),
    ));
    let form = cx.new(|_| AboutForm {
        controller: controller.clone(),
        logo,
        license_open: false,
    });
    window.open_dialog(cx, move |dialog, _, _| framed(dialog).w(px(420.0)).child(form.clone()));
}

/// The component library in reframe's colours.
pub fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |look| {
        let color = |value: u32| -> Hsla { rgb(value).into() };
        look.font_family = theme::ui_font().into();
        look.font_size = px(13.0);
        look.radius = px(theme::RADIUS);
        let colors = &mut look.colors;
        colors.background = color(theme::SURFACE);
        colors.foreground = color(theme::TEXT);
        colors.border = color(theme::BORDER);
        colors.input = color(theme::BORDER_STRONG);
        colors.muted = color(theme::SURFACE_2);
        colors.muted_foreground = color(theme::MUTED);
        colors.popover = color(theme::SURFACE_2);
        colors.popover_foreground = color(theme::TEXT);
        colors.primary = color(theme::ACCENT);
        colors.primary_hover = color(theme::ACCENT).opacity(0.9);
        colors.primary_active = color(theme::ACCENT).opacity(0.8);
        colors.primary_foreground = color(theme::ACCENT_TEXT);
        colors.button_primary = color(theme::ACCENT);
        colors.button_primary_hover = color(theme::ACCENT).opacity(0.9);
        colors.button_primary_active = color(theme::ACCENT).opacity(0.8);
        colors.button_primary_foreground = color(theme::ACCENT_TEXT);
        colors.secondary = color(theme::SURFACE_2);
        colors.secondary_hover = color(theme::BORDER);
        colors.secondary_active = color(theme::BORDER_STRONG);
        colors.secondary_foreground = color(theme::TEXT);
        colors.accent = color(theme::BORDER);
        colors.accent_foreground = color(theme::TEXT);
        colors.ring = color(theme::ACCENT);
        colors.caret = color(theme::TEXT);
        colors.selection = color(theme::ACCENT).opacity(0.35);
        colors.list_active = color(theme::BORDER);
        colors.list_hover = color(theme::BORDER);
    });
}
