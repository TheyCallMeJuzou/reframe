//! Around the windows: the tray icon and its menu, a second start that shows the windows,
//! notifications, the taskbar buttons that flash, and the system going down.
//!
//! Takes the place of the Electron version's `src/main/tray.ts` and of what `app.ts` did with it
//! (`createTray`, `notify`, `revealWindows`, `updateAttention`).

use std::collections::HashMap;

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui_kit::*;
use reframe_core::actions::{is_collapsed, window_sessions};
use reframe_core::i18n::t;
use reframe_core::session_logic::NotifyKind;
use serde_json::json;

use crate::app::{AppEvent, Controller};
use crate::win::{self, TrayEntry, TrayEvent};
use crate::window::{self, Shell};

/// What the entries of the tray menu are known by; a window's entry is this plus its place.
const WINDOW: u32 = 1000;
/// Saved workspaces, by their place in the state.
const WORKSPACE: u32 = 2000;
const NEW_SESSION: u32 = 1;
const NEW_WINDOW: u32 = 2;
const ABOUT: u32 = 3;
const QUIT: u32 = 4;
/// "Restart to update", or in the portable copy "Download reframe X".
const UPDATE: u32 = 5;

/// What update notifications are known by when they are clicked (a session's are known by
/// its id).
const UPDATE_READY_TAG: &str = "update:ready";
const UPDATE_AVAILABLE_TAG: &str = "update:available";

/// The window that takes the tray's messages, for tests to send it what Windows would.
#[derive(Default)]
struct ControlWindow(Option<isize>);

impl Global for ControlWindow {}

/// How many sessions need the user in each window, as its taskbar button's badge says it.
#[derive(Default)]
struct Badges(HashMap<String, usize>);

impl Global for Badges {}

pub fn control_window(cx: &App) -> Option<isize> {
    cx.try_global::<ControlWindow>()?.0
}

/// Sets up the tray icon and everything else around the windows. Call once the windows are
/// open.
pub fn start(controller: &Entity<Controller>, cx: &mut App) {
    let (e2e, key) = {
        let config = controller.read(cx).config();
        (config.e2e, win::profile_key(&config.user_data))
    };
    let (sender, mut events) = unbounded();
    let control = win::create_control(&key, sender);
    cx.set_global(ControlWindow(control));
    win::shut_down_first();
    // A test has no icon in the user's tray; the menu is there all the same.
    if !e2e {
        let added = win::add_tray_icon(
            include_bytes!("../assets/tray.png"),
            include_bytes!("../assets/tray@2x.png"),
        );
        if !added {
            controller.read(cx).log().warn("the tray icon could not be shown", None);
        }
    }
    cx.spawn({
        let controller = controller.clone();
        async move |cx| {
            while let Some(event) = events.next().await {
                cx.update(|cx| handle(&controller, event, cx));
            }
        }
    })
    .detach();

    let app_id = controller.read(cx).config().app_id.clone();
    cx.set_app_identity(&app_id, "reframe");
    cx.on_system_notification_response({
        let controller = controller.clone();
        move |response, cx| {
            let tag = response.tag.as_ref();
            controller
                .read(cx)
                .log()
                .info("notification clicked", Some(json!({ "tag": tag })));
            match tag {
                // The portable copy's new version is downloaded by hand.
                UPDATE_AVAILABLE_TAG => cx.open_url(reframe_core::links::RELEASES_URL),
                UPDATE_READY_TAG => {}
                session => controller.update(cx, |controller, cx| controller.focus_session(session, cx)),
            }
        }
    });

    cx.subscribe(controller, |controller, event, cx| match event {
        AppEvent::Notify(session, kind) => notify(&controller, session, *kind, cx),
        AppEvent::UpdateReady(version) => {
            refresh(&controller, cx);
            notify_update(
                &controller,
                UPDATE_READY_TAG,
                "update.readyTitle",
                "update.readyBody",
                version,
                cx,
            );
        }
        AppEvent::UpdateAvailable(version) => {
            refresh(&controller, cx);
            notify_update(
                &controller,
                UPDATE_AVAILABLE_TAG,
                "update.availableTitle",
                "update.availableBody",
                version,
                cx,
            );
        }
        // Whatever else happens may change what the menu says.
        _ => refresh(&controller, cx),
    })
    .detach();
    refresh(controller, cx);
}

/// Brings the tray's menu and tooltip in step with the windows and their sessions.
fn refresh(controller: &Entity<Controller>, cx: &mut App) {
    let mut badges = None;
    refresh_with(controller, &mut badges, cx);
    if let Some(counts) = badges {
        cx.set_global(Badges(counts));
    }
}

fn refresh_with(controller: &Entity<Controller>, badges: &mut Option<HashMap<String, usize>>, cx: &App) {
    let controller = controller.read(cx);
    let locale = controller.locale();
    let text = |key: &str| t(locale, key, &[]).unwrap_or_default();
    let mut menu: Vec<TrayEntry> = controller
        .state()
        .windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            let labels: Vec<String> = window_sessions(window)
                .iter()
                .filter_map(|id| controller.session_view(id, cx))
                .map(|view| view.label)
                .collect();
            let names = if labels.len() > 3 {
                format!("{}, …", labels[..3].join(", "))
            } else {
                labels.join(", ")
            };
            let mut label = t(locale, "tray.window", &[("n", &(index + 1).to_string())]).unwrap_or_default();
            if !names.is_empty() {
                label = format!("{label} – {names}");
            }
            TrayEntry::Item {
                id: WINDOW + index as u32,
                label,
                // Whether it is on screen (a start by the system leaves all of them in the tray).
                checked: Some(window::shell(&window.id, cx).is_some_and(|shell| !shell.read(cx).is_away())),
                enabled: true,
            }
        })
        .collect();
    let mut workspaces: Vec<TrayEntry> = controller
        .workspace_items()
        .into_iter()
        .enumerate()
        .map(|(index, (_, name, open))| TrayEntry::Item {
            id: WORKSPACE + index as u32,
            label: if open {
                format!("{name} {}", text("workspace.openMark"))
            } else {
                name
            },
            checked: None,
            enabled: true,
        })
        .collect();
    if workspaces.is_empty() {
        workspaces.push(TrayEntry::Item {
            id: 0,
            label: text("workspace.none"),
            checked: None,
            enabled: false,
        });
    }
    let item = |id: u32, key: &str| TrayEntry::Item {
        id,
        label: text(key),
        checked: None,
        enabled: true,
    };
    menu.extend([
        TrayEntry::Separator,
        item(NEW_SESSION, "tray.newSession"),
        item(NEW_WINDOW, "tray.newWindow"),
        TrayEntry::Submenu {
            label: text("tray.workspaces"),
            children: workspaces,
        },
        TrayEntry::Separator,
    ]);
    // Once an update is downloaded; in the portable copy, a newer version to download by hand.
    let update = match (controller.update_ready(), controller.update_available()) {
        (Some(version), _) => t(locale, "tray.restartToUpdate", &[("version", version)]),
        (None, Some(version)) => t(locale, "tray.downloadUpdate", &[("version", version)]),
        (None, None) => None,
    };
    if let Some(label) = update {
        menu.push(TrayEntry::Item {
            id: UPDATE,
            label,
            checked: None,
            enabled: true,
        });
    }
    menu.extend([item(ABOUT, "tray.about"), item(QUIT, "tray.quit")]);
    let tooltip = match controller.waiting_total(cx) {
        0 => text("tray.tooltip"),
        1 => text("title.waitingOne"),
        n => t(locale, "title.waiting", &[("n", &n.to_string())]).unwrap_or_default(),
    };
    win::set_tray(&tooltip, menu);

    // A badge on the taskbar button of every window with sessions that need the user.
    let waiting_text = |count: usize| match count {
        1 => text("title.waitingOne"),
        n => t(locale, "title.waiting", &[("n", &n.to_string())]).unwrap_or_default(),
    };
    let counts: Vec<(String, usize)> = controller
        .state()
        .windows
        .iter()
        .map(|window| (window.id.clone(), controller.waiting_in(window, cx)))
        .collect();
    let off = controller.config().env.contains_key("REFRAME_DISABLE_NOTIFICATIONS");
    let shown = cx
        .try_global::<Badges>()
        .map(|badges| badges.0.clone())
        .unwrap_or_default();
    for (id, count) in &counts {
        if shown.get(id).copied().unwrap_or(0) == *count || off {
            continue;
        }
        if let Some(hwnd) = window::shell(id, cx).and_then(|shell| shell.read(cx).hwnd()) {
            let description = (*count > 0).then(|| waiting_text(*count));
            cx.foreground_executor()
                .spawn(async move { win::set_badge(hwnd, description.as_deref()) })
                .detach();
        }
    }
    badges.replace(counts.into_iter().collect());
}

fn first_shell(controller: &Entity<Controller>, cx: &App) -> Option<Entity<Shell>> {
    let id = controller.read(cx).state().windows.first()?.id.clone();
    window::shell(&id, cx)
}

/// Shows the first window and does something in it.
fn in_first_window(
    controller: &Entity<Controller>,
    cx: &mut App,
    run: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
) {
    let Some(shell) = first_shell(controller, cx) else {
        return;
    };
    let handle = shell.read(cx).handle();
    let _ = handle.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.show(true, cx);
            run(shell, window, cx);
        });
    });
}

/// Shows the windows again, when reframe is started a second time: the same ones a start
/// shows, so windows the user closed stay in the tray.
pub fn reveal_windows(controller: &Entity<Controller>, cx: &mut App) {
    let ids: Vec<String> = {
        let windows = &controller.read(cx).state().windows;
        let open: Vec<String> = windows
            .iter()
            .filter(|window| !window.hidden)
            .map(|window| window.id.clone())
            .collect();
        if open.is_empty() {
            windows.iter().take(1).map(|window| window.id.clone()).collect()
        } else {
            open
        }
    };
    for id in ids {
        if let Some(shell) = window::shell(&id, cx) {
            shell.update(cx, |shell, cx| shell.show(true, cx));
        }
    }
}

/// What the tray icon, its menu, a second start or the system ask for.
pub fn handle(controller: &Entity<Controller>, event: TrayEvent, cx: &mut App) {
    match event {
        TrayEvent::Clicked => in_first_window(controller, cx, |_, _, _| {}),
        TrayEvent::Command(NEW_SESSION) => {
            in_first_window(controller, cx, |shell, window, cx| shell.open_new_session(window, cx));
        }
        TrayEvent::Command(ABOUT) => {
            in_first_window(controller, cx, |shell, window, cx| shell.open_about(window, cx));
        }
        TrayEvent::Command(NEW_WINDOW) => {
            if let Some(shell) = first_shell(controller, cx) {
                shell.update(cx, |shell, cx| shell.new_window(cx));
            }
        }
        TrayEvent::Command(QUIT) => controller.update(cx, |controller, cx| controller.shutdown(cx)),
        TrayEvent::Command(UPDATE) => {
            if controller.read(cx).update_ready().is_some() {
                controller.update(cx, |controller, cx| controller.restart_to_update(cx));
            } else if controller.read(cx).update_available().is_some() {
                cx.open_url(reframe_core::links::RELEASES_URL);
            }
        }
        TrayEvent::Command(id) if id >= WORKSPACE => {
            let index = (id - WORKSPACE) as usize;
            let workspace = controller
                .read(cx)
                .workspace_items()
                .get(index)
                .map(|(id, ..)| id.clone());
            if let Some(workspace) = workspace {
                controller.update(cx, |controller, cx| controller.open_workspace(&workspace, cx));
            }
        }
        // A window's entry: shown when it is hidden, hidden when it is shown.
        TrayEvent::Command(id) => {
            let index = id.saturating_sub(WINDOW) as usize;
            let window = controller.read(cx).state().windows.get(index).cloned();
            let Some(shell) = window.as_ref().and_then(|window| window::shell(&window.id, cx)) else {
                return;
            };
            shell.update(cx, |shell, cx| {
                if shell.is_away() {
                    shell.show(true, cx);
                } else {
                    shell.put_away(cx);
                }
            });
        }
        TrayEvent::Reveal { build } => {
            let name = |build: usize| if build == 2 { "installed" } else { "development" };
            controller.read(cx).log().info(
                "reframe was started again, showing its windows",
                Some(json!({ "build": name(build) })),
            );
            reveal_windows(controller, cx);
            // The installed reframe started while a development build runs, or the other way
            // round: it says so, or one ends up trying out the build that is not running.
            let own = controller.read(cx).config().copy.build();
            if build != own && (build == 1 || build == 2) {
                other_build_started(controller, name(build), name(own), cx);
            }
        }
        TrayEvent::SystemEnding => controller.update(cx, |controller, cx| controller.system_ending(cx)),
        TrayEvent::Quit => {
            controller
                .read(cx)
                .log()
                .info("the installer asks reframe to quit", None);
            controller.update(cx, |controller, cx| controller.shutdown(cx));
        }
    }
}

/// Another build of reframe (installed or development) was started for the same profile; this
/// one keeps running and says so.
fn other_build_started(controller: &Entity<Controller>, started: &str, running: &str, cx: &mut App) {
    let (locale, first) = {
        let controller = controller.read(cx);
        controller.log().info(
            "another build was started, this one keeps running",
            Some(json!({ "started": started, "running": running })),
        );
        let first = controller.state().windows.first().map(|window| window.id.clone());
        (controller.locale(), first)
    };
    let Some(first) = first else { return };
    let text = |key: &str, params: &[(&str, &str)]| t(locale, key, params).unwrap_or_default();
    let started = text(&format!("otherBuild.{started}"), &[]);
    let running = text(&format!("otherBuild.{running}"), &[]);
    let question = window::Question {
        title: text("otherBuild.title", &[]),
        message: text("otherBuild.message", &[("started", &started), ("running", &running)]),
        detail: Some(text("otherBuild.detail", &[])),
        confirm: text("about.close", &[]),
        cancel: None,
        danger: false,
    };
    drop(window::ask(&first, question, cx));
}

/// An update was downloaded, or (portable copy) a newer version is out.
fn notify_update(
    controller: &Entity<Controller>,
    tag: &'static str,
    title: &str,
    body: &str,
    version: &str,
    cx: &mut App,
) {
    let controller = controller.read(cx);
    if controller.config().env.contains_key("REFRAME_DISABLE_NOTIFICATIONS") {
        return;
    }
    let locale = controller.locale();
    let params = [("version", version)];
    cx.show_system_notification(SystemNotification {
        tag: tag.into(),
        title: t(locale, title, &params).unwrap_or_default().into(),
        body: t(locale, body, &params).unwrap_or_default().into(),
        actions: Vec::new(),
    });
}

/// A session is done or needs the user: a notification says so, unless the user is looking at
/// its tile; one that waits where nobody looks also has its window's taskbar button flash.
fn notify(controller: &Entity<Controller>, session_id: &str, kind: NotifyKind, cx: &mut App) {
    let found = {
        let controller = controller.read(cx);
        controller.window_of(session_id).and_then(|window| {
            let view = controller.session_view(session_id, cx)?;
            Some((window.id.clone(), is_collapsed(window, session_id), view.label))
        })
    };
    let Some((window_id, collapsed, label)) = found else {
        return;
    };
    let (locale, log, off) = {
        let controller = controller.read(cx);
        let off = controller.config().env.contains_key("REFRAME_DISABLE_NOTIFICATIONS");
        (controller.locale(), controller.log().clone(), off)
    };
    let kind_name = match kind {
        NotifyKind::Done => "done",
        NotifyKind::Waiting => "waiting",
    };
    let skip = |reason: &str| {
        log.info(
            "notification skipped",
            Some(json!({ "session": session_id, "kind": kind_name, "reason": reason })),
        );
    };
    let shell = window::shell(&window_id, cx);
    let seen = shell.as_ref().is_some_and(|shell| shell.read(cx).is_attentive()) && !collapsed;
    if kind == NotifyKind::Waiting
        && !seen
        && !off
        && let Some(shell) = &shell
    {
        shell.read(cx).flash(cx);
    }
    if off {
        return skip("notifications off");
    }
    if seen {
        return skip("window in focus, tile visible");
    }
    let (title, body) = match kind {
        NotifyKind::Done => ("notify.done", "notify.doneBody"),
        NotifyKind::Waiting => ("notify.waiting", "notify.waitingBody"),
    };
    cx.show_system_notification(SystemNotification {
        tag: session_id.to_owned().into(),
        title: t(locale, title, &[("label", &label)]).unwrap_or_default().into(),
        body: t(locale, body, &[]).unwrap_or_default().into(),
        actions: Vec::new(),
    });
    log.info(
        "notification shown",
        Some(json!({ "session": session_id, "kind": kind_name })),
    );
}
