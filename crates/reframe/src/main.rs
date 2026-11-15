// Release builds are a window application: no console window opens next to it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod colors;
mod console;
mod dialogs;
mod e2e;
mod find;
mod foreign;
mod glyphs;
mod input;
mod keys;
mod process;
mod pty;
mod sessions;
mod terminal;
mod terminal_view;
mod theme;
mod tray;
mod watcher;
mod win;
mod window;
mod wsl;

use gpui_kit::*;

use app::{Config, Controller, NewSessionRequest, StartError};

struct Args {
    /// Folders to start a new session in, once the app is up. A stand-in for the New session
    /// dialog until the window has one.
    new: Vec<String>,
    /// Started by the system when the user logged in: reframe stays in the tray.
    autostart: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        new: Vec::new(),
        autostart: false,
    };
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--new" => args.new.extend(input.next()),
            app::AUTOSTART_ARG => args.autostart = true,
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    args
}

fn main() {
    // A copy of reframe started to type into another console: it does only that.
    if let Some(code) = console::run_from_args() {
        std::process::exit(code);
    }
    // A copy of reframe started through a WSL session to tell which program started that session.
    if let Some(code) = wsl::print_parent_pid_from_args() {
        std::process::exit(code);
    }
    pty::prepare_environment();
    let args = parse_args();
    let config = Config::from_environment();

    // One reframe per profile: the running one shows its windows and this one leaves.
    let profile = win::profile_key(&config.user_data);
    if !win::claim_profile(&profile) {
        let build = if cfg!(debug_assertions) {
            "development"
        } else {
            "installed"
        };
        reframe_core::log::Logger::new(config.user_data.join("logs")).info(
            "reframe already runs, showing its windows",
            Some(serde_json::json!({ "build": build })),
        );
        // It may still be starting.
        for _ in 0..50 {
            if win::reveal_running(&profile, if cfg!(debug_assertions) { 1 } else { 2 }) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return;
    }

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        dialogs::apply_theme(cx);
        // reframe says when it quits: after its sessions were ended, not when a window goes.
        cx.set_quit_mode(QuitMode::Explicit);
        let e2e = config.e2e;
        let controller = cx.new(|cx| Controller::new(config, cx).unwrap_or_else(|error| report(&error)));

        let orphans = controller.update(cx, |controller, cx| controller.cleanup_orphans(cx));
        cx.spawn(async move |cx| {
            orphans.await;
            cx.update(|cx| {
                let windows: Vec<(String, bool)> = {
                    let state = controller.read(cx).state();
                    // The windows that were open (not closed into the tray); the first one if all were.
                    let any_open = state.windows.iter().any(|window| !window.hidden);
                    // Started by the system, all of them stay in the tray.
                    let autostart = args.autostart;
                    let shown = |index: usize, hidden: bool| !autostart && if any_open { !hidden } else { index == 0 };
                    state
                        .windows
                        .iter()
                        .enumerate()
                        .map(|(index, window)| (window.id.clone(), shown(index, window.hidden)))
                        .collect()
                };
                for (id, shown) in &windows {
                    window::open(&controller, id, *shown, cx);
                }
                window::init(&controller, cx);
                tray::start(&controller, cx);
                foreign::start(&controller, cx);
                controller.update(cx, |controller, cx| {
                    controller.resume_sessions(cx);
                    let first = windows.first().map(|(id, _)| id.clone()).unwrap_or_default();
                    for folder in &args.new {
                        let request = NewSessionRequest {
                            cwd: folder.clone(),
                            ..NewSessionRequest::default()
                        };
                        if let Err(error) = controller.create_session(&first, request, cx) {
                            eprintln!("no session in {folder}: {}", error.key());
                        }
                    }
                });
                if e2e {
                    e2e::start(controller.clone(), cx);
                }
            });
        })
        .detach();
    });
}

/// Says why reframe does not start, and exits.
fn report(error: &StartError) -> ! {
    match error {
        StartError::NewerState(file) => {
            eprintln!("{} was saved by a newer reframe; it is left as it is.", file.display());
        }
        StartError::Hooks(error) => eprintln!("could not listen for Claude Code's hooks: {error}"),
    }
    std::process::exit(1)
}
