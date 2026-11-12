// Release builds are a window application: no console window opens next to it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod colors;
mod e2e;
mod glyphs;
mod input;
mod process;
mod pty;
mod sessions;
mod terminal;
mod terminal_view;
mod window;

use gpui_kit::*;

use app::{Config, Controller, NewSessionRequest, StartError};

struct Args {
    /// Folders to start a new session in, once the app is up. A stand-in for the New session
    /// dialog until the window has one.
    new: Vec<String>,
}

fn parse_args() -> Args {
    let mut args = Args { new: Vec::new() };
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--new" => args.new.extend(input.next()),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    args
}

fn main() {
    pty::prepare_environment();
    let args = parse_args();
    let config = Config::from_environment();

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        // reframe says when it quits: after its sessions were ended, not when a window goes.
        cx.set_quit_mode(QuitMode::Explicit);
        let e2e = config.e2e;
        let controller = cx.new(|cx| Controller::new(config, cx).unwrap_or_else(|error| report(&error)));

        // Closing the last window quits (until there is a tray to stay in).
        cx.on_window_closed({
            let controller = controller.clone();
            move |cx, _| {
                if cx.windows().is_empty() {
                    controller.update(cx, |controller, cx| controller.shutdown(cx));
                }
            }
        })
        .detach();

        let orphans = controller.update(cx, |controller, cx| controller.cleanup_orphans(cx));
        cx.spawn(async move |cx| {
            orphans.await;
            cx.update(|cx| {
                let windows: Vec<(String, bool)> = {
                    let state = controller.read(cx).state();
                    // The windows that were open (not closed into the tray); the first one if all were.
                    let any_open = state.windows.iter().any(|window| !window.hidden);
                    let shown = |index: usize, hidden: bool| if any_open { !hidden } else { index == 0 };
                    state
                        .windows
                        .iter()
                        .enumerate()
                        .map(|(index, window)| (window.id.clone(), shown(index, window.hidden)))
                        .collect()
                };
                for (id, shown) in &windows {
                    window::open(&controller, id, *shown && !e2e, cx);
                }
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
