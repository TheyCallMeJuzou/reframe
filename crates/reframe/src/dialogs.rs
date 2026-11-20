//! The New session, Settings and About dialogs.
//!
//! They take the place of the Electron version's `NewSessionDialog.tsx`, `SettingsDialog.tsx`
//! and `AboutDialog.tsx`. Each is a view of its own inside the component library's dialog, which
//! brings the backdrop, Escape and the focus trap.

use std::cell::Cell;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{Disableable as _, IndexPath, Theme, ThemeMode, WindowExt as _};
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
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(rgb(if selected { theme::ACCENT } else { theme::BORDER_STRONG }))
        .text_size(px(12.0))
        .cursor_pointer()
        .hover(|style| style.border_color(rgb(theme::MUTED)))
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
        self.busy = true;
        self.error = None;
        cx.notify();
        let handle = window.window_handle();
        self.pending = Some(cx.spawn(async move |this, cx| {
            let created = created.await;
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
        let mut form = div().flex().flex_col().gap(px(10.0)).child(field(
            text("dialog.folder"),
            div()
                .flex()
                .gap(px(6.0))
                .child(div().flex_1().min_w_0().child(Input::new(&self.folder)))
                .child(
                    Button::new("browse")
                        .label(text("dialog.browse"))
                        .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))),
                ),
        ));
        if !self.recent.is_empty() {
            let mut recent = div().flex().flex_wrap().gap(px(6.0));
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
            .child(field(text("dialog.name"), Input::new(&self.name)))
            .child(
                div().pl(px(4.0)).child(
                    Checkbox::new("name-from-folder")
                        .label(text("dialog.nameFromFolder"))
                        .checked(self.name_from_folder)
                        .on_click(cx.listener(|this, on: &bool, window, cx| {
                            this.set_name_from_folder(*on, window, cx);
                        })),
                ),
            )
            .child(
                Checkbox::new("worktree")
                    .label(text("dialog.worktree"))
                    .checked(self.worktree)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.worktree = *on;
                        cx.notify();
                    })),
            );
        if self.worktree {
            form = form.child(
                div()
                    .pl(px(4.0))
                    .child(field(text("dialog.worktreeName"), Input::new(&self.worktree_name))),
            );
        }
        form = form.child(
            div()
                .id("advanced")
                .flex()
                .items_center()
                .gap(px(6.0))
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
            let mut suggestions = div().flex().flex_wrap().gap(px(6.0));
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
                .child(field(text("dialog.model"), Input::new(&self.model)))
                .child(suggestions)
                .child(field(text("dialog.permissionMode"), Select::new(&self.permission)));
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
            form = form.child(div().text_color(rgb(theme::DANGER)).child(message));
        }
        form.child(
            div()
                .mt(px(6.0))
                .flex()
                .justify_end()
                .gap(px(8.0))
                .child(
                    Button::new("cancel")
                        .label(text("dialog.cancel"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("start")
                        .primary()
                        .label(text("dialog.start"))
                        .disabled(folder.trim().is_empty() || self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                ),
        )
    }
}

pub fn open_new_session(controller: &Entity<Controller>, window_id: &str, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| NewSessionForm::new(controller.clone(), window_id.to_owned(), window, cx));
    let controller = controller.clone();
    let focus = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if focus.replace(false) {
            form.read(cx).folder.focus_handle(cx).focus(window, cx);
        }
        let title = words(controller.read(cx).locale())("dialog.title");
        dialog.title(title).w(px(520.0)).child(form.clone())
    });
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

    /// The three lists to choose from, in the current language, showing what is set.
    fn choices(controller: &Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Choices {
        let (locale, system, preferences) = {
            let controller = controller.read(cx);
            (
                controller.locale(),
                controller.config().system_locale.clone(),
                controller.state().preferences.clone(),
            )
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
            permission_choices(text("dialog.default")),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let step = |id: &'static str, label: String, change: TextSizeChange, cx: &mut Context<Self>| {
            Button::new(id)
                .label(label)
                .disabled(font_size_after(size, change) == size)
                .on_click(cx.listener(move |this, _, _, cx| this.text_size(change, cx)))
        };
        let text_size = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(step("smaller", "−".to_owned(), TextSizeChange::Smaller, cx))
            .child(div().w(px(24.0)).text_center().child(size.to_string()))
            .child(step("bigger", "+".to_owned(), TextSizeChange::Bigger, cx))
            .child(step("normal", text("settings.normal"), TextSizeChange::Reset, cx));
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
                    Button::new("claude-path")
                        .label(text("settings.claudeUsePath"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set(cx, |preferences| preferences.claude_bin = None);
                        })),
                )
            })
            .child(
                Button::new("claude-choose")
                    .label(text("settings.claudeChoose"))
                    .on_click(cx.listener(|this, _, window, cx| this.choose_claude(window, cx))),
            );
        let claude_state = text(match (&chosen, &claude) {
            (Some(_), Some(_)) => "settings.claudeChosen",
            (Some(_), None) => "settings.claudeMissing",
            (None, Some(_)) => "settings.claudeFound",
            (None, None) => "settings.claudeNotFound",
        });
        let claude_path = chosen.or(claude).unwrap_or_default();

        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(setting(
                text("settings.language"),
                div().w(px(220.0)).child(Select::new(&self.language)),
            ))
            .child(setting(text("settings.textSize"), text_size))
            .child(hint(size_hint))
            .child(
                Checkbox::new("snap")
                    .label(text("settings.snap"))
                    .checked(preferences.snap_to_grid)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        let on = *on;
                        this.set(cx, move |preferences| preferences.snap_to_grid = on);
                    })),
            )
            .child(hint(text("settings.snapHint")))
            .child(
                Checkbox::new("autostart")
                    .label(text("settings.autostart"))
                    .checked(self.controller.read(cx).starts_with_system())
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        let on = *on;
                        this.controller.update(cx, |controller, cx| {
                            controller.set_start_with_system(on, cx);
                        });
                        cx.notify();
                    })),
            )
            .child(hint(text("settings.autostartHint")))
            .child(setting(
                text("settings.closeWindow"),
                div().w(px(220.0)).child(Select::new(&self.close_window)),
            ))
            .child(hint(text("settings.closeWindowHint")))
            .child(setting(text("settings.claude"), claude_buttons))
            .child(hint(format!("{claude_state} {claude_path}")))
            .child(hint(text("settings.claudeHint")))
            .child(setting(
                text("settings.defaultModel"),
                div().w(px(220.0)).child(Input::new(&self.model)),
            ))
            .child(setting(
                text("settings.defaultPermissionMode"),
                div().w(px(220.0)).child(Select::new(&self.permission)),
            ))
            .child(hint(text("settings.launchHint")))
            // Only where this copy checks at all.
            .when(self.controller.read(cx).update_checks(), |form| {
                form.child(
                    Checkbox::new("updates")
                        .label(text("settings.updates"))
                        .checked(preferences.check_for_updates)
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            let on = *on;
                            this.set(cx, move |preferences| preferences.check_for_updates = on);
                        })),
                )
                .child(hint(text("settings.updatesHint")))
            })
            .child(
                div().mt(px(6.0)).flex().justify_end().child(
                    Button::new("close")
                        .primary()
                        .label(text("settings.close"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    }
}

pub fn open_settings(controller: &Entity<Controller>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| SettingsForm::new(controller.clone(), window, cx));
    let controller = controller.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let title = words(controller.read(cx).locale())("settings.title");
        dialog.title(title).w(px(560.0)).child(form.clone())
    });
}

// ---------- closing a window ----------

/// Asks what to do with the sessions of a window that is being closed: end them, keep them
/// running, or leave the window open. The answer can be kept.
pub struct CloseForm {
    shell: WeakEntity<Shell>,
    controller: Entity<Controller>,
    /// How many sessions are in the window.
    count: usize,
    remember: bool,
}

impl CloseForm {
    fn answer(&mut self, choice: Option<CloseWindow>, window: &mut Window, cx: &mut Context<Self>) {
        let remember = self.remember;
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.answer_close(choice, remember, window, cx));
    }
}

impl Render for CloseForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.controller.read(cx).locale();
        let text = words(locale);
        let message = if self.count == 1 {
            text("closeWindow.messageOne")
        } else {
            t(locale, "closeWindow.message", &[("count", &self.count.to_string())]).unwrap_or_default()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(message))
            .child(div().text_color(rgb(theme::MUTED)).child(text("closeWindow.detail")))
            .child(
                Checkbox::new("remember")
                    .label(text("closeWindow.remember"))
                    .checked(self.remember)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.remember = *on;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .mt(px(6.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(Button::new("end").label(text("closeWindow.end")).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.answer(Some(CloseWindow::End), window, cx);
                        },
                    )))
                    .child(
                        Button::new("keep")
                            .primary()
                            .label(text("closeWindow.keep"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.answer(Some(CloseWindow::Keep), window, cx);
                            })),
                    )
                    .child(
                        Button::new("cancel")
                            .label(text("dialog.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.answer(None, window, cx))),
                    ),
            )
    }
}

pub fn open_close_window(
    shell: WeakEntity<Shell>,
    controller: &Entity<Controller>,
    count: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|_| CloseForm {
        shell,
        controller: controller.clone(),
        count,
        remember: false,
    });
    let controller = controller.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let title = words(controller.read(cx).locale())("closeWindow.title");
        dialog.title(title).w(px(480.0)).child(form.clone())
    });
}

// ---------- a question ----------

/// A question with one or two answers (taking over a session that runs elsewhere, deleting a
/// workspace, something that did not work). The answer goes to the window that asked.
pub struct QuestionForm {
    shell: WeakEntity<Shell>,
    question: crate::window::Question,
}

impl QuestionForm {
    fn answer(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.answer_question(yes, window, cx));
    }
}

impl Render for QuestionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let question = &self.question;
        let mut buttons = div().mt(px(6.0)).flex().justify_end().gap(px(8.0));
        if let Some(cancel) = &question.cancel {
            buttons = buttons.child(
                Button::new("cancel")
                    .label(cancel.clone())
                    .on_click(cx.listener(|this, _, window, cx| this.answer(false, window, cx))),
            );
        }
        let confirm = Button::new("confirm")
            .label(question.confirm.clone())
            .on_click(cx.listener(|this, _, window, cx| this.answer(true, window, cx)));
        // Something that cannot be undone is not the default.
        buttons = buttons.child(if question.danger {
            confirm.danger()
        } else {
            confirm.primary()
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(question.message.clone()))
            .when_some(question.detail.clone(), |form, detail| {
                form.child(div().text_color(rgb(theme::MUTED)).child(detail))
            })
            .child(buttons)
    }
}

pub fn open_question(shell: WeakEntity<Shell>, question: crate::window::Question, window: &mut Window, cx: &mut App) {
    let title = question.title.clone();
    let form = cx.new(|_| QuestionForm { shell, question });
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title(title.clone()).w(px(460.0)).child(form.clone())
    });
}

// ---------- taking over from a terminal window ----------

/// Confirms taking over the session(s) of a terminal window that was dropped onto this window;
/// with several, which ones.
pub struct TerminalTakeoverForm {
    shell: WeakEntity<Shell>,
    controller: Entity<Controller>,
    candidates: Vec<crate::window::TerminalCandidate>,
    picked: Vec<String>,
}

impl TerminalTakeoverForm {
    fn answer(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = confirmed.then(|| self.picked.clone());
        let _ = self
            .shell
            .update(cx, |shell, cx| shell.answer_terminal(chosen, window, cx));
    }
}

impl Render for TerminalTakeoverForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = words(self.controller.read(cx).locale());
        let single = self.candidates.len() == 1;
        let mut list = div().flex().flex_col().gap(px(6.0));
        for candidate in &self.candidates {
            let id = candidate.claude_session_id.clone();
            let label = candidate.name.clone().unwrap_or_else(|| folder_name(&candidate.cwd));
            let entry = div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::MUTED))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(candidate.cwd.clone()),
                );
            list = list.child(if single {
                div().child(entry)
            } else {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
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
            .gap(px(10.0))
            .child(div().child(text(if single { "foreign.single" } else { "foreign.multiple" })))
            .child(list)
            .child(hint(text("foreign.detail")))
            .child(
                div()
                    .mt(px(6.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        Button::new("cancel")
                            .label(text("dialog.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.answer(false, window, cx))),
                    )
                    .child(
                        Button::new("confirm")
                            .primary()
                            .label(text("takeover.confirm"))
                            .disabled(self.picked.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| this.answer(true, window, cx))),
                    ),
            )
    }
}

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
        let title = words(controller.read(cx).locale())("foreign.title");
        dialog.title(title).w(px(480.0)).child(form.clone())
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
            .child(field(text("workspace.nameLabel"), Input::new(&self.name)))
            .child(
                div()
                    .mt(px(6.0))
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(
                        Button::new("cancel")
                            .label(text("dialog.cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save")
                            .primary()
                            .label(text("workspace.saveButton"))
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
    let focus = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if focus.replace(false) {
            let name = form.read(cx).name.clone();
            name.update(cx, |name, cx| {
                name.focus(window, cx);
                name.select_all(window, cx);
            });
        }
        let title = words(form.read(cx).controller.read(cx).locale())("workspace.nameTitle");
        dialog.title(title).w(px(460.0)).child(form.clone())
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
                .text_color(rgb(theme::ACCENT))
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(move |_, _, cx| cx.open_url(url))
                .child(label)
        };
        let version = t(locale, "about.version", &[("version", env!("CARGO_PKG_VERSION"))]).unwrap_or_default();
        let mut about = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0))
            .child(img(self.logo.clone()).size(px(72.0)))
            .child(
                div()
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("reframe"),
            )
            .child(div().text_color(rgb(theme::MUTED)).child(version))
            .child(div().text_center().child(text("about.tagline")))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(16.0))
                    .child(link("github", text("about.github"), REPO_URL))
                    .child(link("star", format!("⭐ {}", text("about.star")), REPO_URL)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(4.0))
                    .text_color(rgb(theme::MUTED))
                    .child(text("about.madeWith"))
                    .child(link("author", "TheyCallMeJuzou".to_owned(), AUTHOR_URL)),
            )
            .child(
                div()
                    .id("license")
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
                .w_full()
                .max_h(px(180.0))
                .overflow_y_scroll()
                .p(px(8.0))
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::BG))
                .text_size(px(11.0))
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
            div().mt(px(6.0)).child(
                Button::new("close")
                    .primary()
                    .label(text("about.close"))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ),
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
    window.open_dialog(cx, move |dialog, _, _| dialog.w(px(440.0)).child(form.clone()));
}

/// The component library in reframe's colours.
pub fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |look| {
        let color = |value: u32| -> Hsla { rgb(value).into() };
        look.font_family = theme::UI_FONT.into();
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
