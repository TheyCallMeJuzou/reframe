// Release builds are a window application: no console window opens next to it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod browser;
mod clipboard;
mod colors;
#[cfg(windows)]
mod console;
mod dialogs;
mod e2e;
mod find;
#[cfg(windows)]
mod foreign;
mod glyphs;
mod http;
mod icons;
mod input;
mod keys;
#[cfg(windows)]
mod ole_drop;
mod platform;
mod process;
mod pty;
mod sessions;
mod takeover;
mod terminal;
mod terminal_view;
mod theme;
mod tray;
mod updates;
mod watcher;
mod window;
mod wsl;

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::*;
use reframe_core::i18n::{Locale, resolve_locale, t};
use reframe_core::log::Logger;

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
            // 0.x's launchers on Linux (its AppImage's desktop entry, AppImageLauncher's copy of it)
            // start it with Chromium's `--no-sandbox`, and with the files of `%U`: neither means
            // anything to this reframe.
            other if cfg!(not(windows)) => eprintln!("argument ignored: {other}"),
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
    #[cfg(windows)]
    if let Some(code) = console::run_from_args() {
        std::process::exit(code);
    }
    // A copy of reframe started through a WSL session to tell which program started that session.
    if let Some(code) = wsl::print_parent_pid_from_args() {
        std::process::exit(code);
    }
    // Started by the installer: asks a running reframe to quit, or marks a shortcut as reframe's.
    if let Some(code) = installer_helper() {
        std::process::exit(code);
    }
    pty::prepare_environment();
    let args = parse_args();
    let config = Config::from_environment();
    log_panics(&config);

    // One reframe per profile: the running one shows its windows and this one leaves.
    let profile = platform::profile_key(&config.user_data);
    if !platform::claim_profile(&profile) {
        let build = config.copy.build();
        reframe_core::log::Logger::new(config.user_data.join("logs")).info(
            "reframe already runs, showing its windows",
            Some(serde_json::json!({ "build": if build == 1 { "development" } else { "installed" } })),
        );
        // It may still be starting.
        for _ in 0..50 {
            if platform::reveal_running(&profile, build) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return;
    }

    let start_failed = StartFailed::of(&config);
    // reframe 0.x on Linux keeps its lock elsewhere: it must not run on the same state file. It
    // may be on its way out: its updater starts this one while it quits ("Restart to update").
    #[cfg(not(windows))]
    {
        let older = || platform::older_reframe_running(&config.user_data);
        for _ in 0..50 {
            if older().is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if let Some(pid) = older() {
            start_failed.log.warn(
                "reframe 0.x runs on this profile",
                Some(serde_json::json!({ "pid": pid })),
            );
            let text = |key: &str| t(start_failed.locale, key, &[]).unwrap_or_default();
            start_failed.show(&text("olderRunning.title"), &text("olderRunning.message"));
            std::process::exit(1);
        }
    }
    // The component library's own icons: the arrow of a choice box, the check mark, clearing a field.
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::choose_fonts(cx);
            // A picture dragged out of a browser exists only in the drag: written there when it is
            // dropped, and cleared with the other dropped files.
            #[cfg(windows)]
            gpui_windows::set_dropped_files_folder(reframe_core::cleanup::drops_dir());
            dialogs::apply_theme(cx);
            // reframe says when it quits: after its sessions were ended, not when a window goes.
            cx.set_quit_mode(QuitMode::Explicit);
            // Under test, links are caught instead of opened, and nothing moves (dialogs come and go
            // in one frame), as on a machine with Windows' animations off and on CI.
            if config.e2e {
                browser::catch_links(cx);
                cx.set_reduce_motion(true);
            }
            let e2e = config.e2e;
            let controller =
                cx.new(|cx| Controller::new(config, cx).unwrap_or_else(|error| start_failed.report(&error)));

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
                        let shown =
                            |index: usize, hidden: bool| !autostart && if any_open { !hidden } else { index == 0 };
                        state
                            .windows
                            .iter()
                            .enumerate()
                            .map(|(index, window)| (window.id.clone(), shown(index, window.hidden)))
                            .collect()
                    };
                    // On Linux whether there is a tray decides how a window waits out of sight
                    // (see `window::open`): the tray comes first there.
                    #[cfg(not(windows))]
                    tray::start(&controller, cx);
                    for (id, shown) in &windows {
                        window::open(&controller, id, *shown, cx);
                    }
                    window::init(&controller, cx);
                    #[cfg(windows)]
                    tray::start(&controller, cx);
                    #[cfg(not(windows))]
                    tray::refresh(&controller, cx);
                    #[cfg(windows)]
                    foreign::start(&controller, cx);
                    controller.update(cx, |controller, cx| {
                        controller.resume_sessions(cx);
                        controller.start_update_checks(cx);
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
                    STARTED.store(true, Ordering::Release);
                });
            })
            .detach();
        });
}

/// What the installer has reframe do: `--quit` asks the reframe that runs for this copy's
/// profile to quit (0 when one was asked, 1 when none runs); `--shortcut-app-id <link> <id>`
/// gives a shortcut the id Windows knows reframe by. `None` for a normal start.
fn installer_helper() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--quit") => {
            let config = Config::from_environment();
            Some(
                if platform::ask_running_to_quit(&platform::profile_key(&config.user_data)) {
                    0
                } else {
                    1
                },
            )
        }
        #[cfg(windows)]
        Some("--shortcut-app-id") => {
            let (Some(link), Some(id)) = (args.get(1), args.get(2)) else {
                return Some(2);
            };
            Some(match platform::set_shortcut_app_id(std::path::Path::new(link), id) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("{link}: {error}");
                    1
                }
            })
        }
        _ => None,
    }
}

/// Set once reframe is up: a panic before that is a start that failed.
static STARTED: AtomicBool = AtomicBool::new(false);

/// What a start that fails needs to say why, in the language of the system (the user's choice is
/// in the state, which may be what failed).
#[derive(Clone)]
struct StartFailed {
    locale: Locale,
    log: Logger,
    /// The end-to-end tests are not shown a dialog.
    quiet: bool,
}

impl StartFailed {
    fn of(config: &Config) -> StartFailed {
        StartFailed {
            locale: resolve_locale(&config.system_locale),
            log: Logger::new(config.user_data.join("logs")),
            quiet: config.e2e,
        }
    }

    /// Says why reframe does not start (in the log and, unless a test drives it, in a message
    /// box), and exits.
    fn report(&self, error: &StartError) -> ! {
        let text = |key: &str, params: &[(&str, &str)]| t(self.locale, key, params).unwrap_or_default();
        match error {
            // Logged by the controller already.
            StartError::NewerState(file) => {
                let file = file.display().to_string();
                self.show(
                    &text("state.newerTitle", &[]),
                    &text("state.newerMessage", &[("file", &file)]),
                );
            }
            StartError::Hooks(error) => self.failed(&format!("could not listen for Claude Code's hooks: {error}")),
            StartError::State(file, error) => {
                self.failed(&format!("{} could not be read: {error}", file.display()));
            }
        }
        std::process::exit(1)
    }

    /// Something else went wrong while starting.
    fn failed(&self, error: &str) {
        self.log
            .error("reframe could not start", Some(serde_json::json!({ "error": error })));
        let log = self.log.file().display().to_string();
        let message = t(self.locale, "startFailed.message", &[("error", error), ("log", &log)]).unwrap_or_default();
        self.show(&t(self.locale, "startFailed.title", &[]).unwrap_or_default(), &message);
    }

    fn show(&self, title: &str, message: &str) {
        eprintln!("{title}: {message}");
        if !self.quiet {
            platform::error_box(title, message);
        }
    }
}

/// Every panic goes to the log; one before reframe is up is a start that failed, which the user
/// is told about (a release build has no console to print it on).
fn log_panics(config: &Config) {
    let start_failed = StartFailed::of(config);
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let error = info.to_string();
        if STARTED.load(Ordering::Acquire) {
            start_failed
                .log
                .error("panic", Some(serde_json::json!({ "error": error })));
        } else {
            start_failed.failed(&error);
        }
        default(info);
    }));
}
