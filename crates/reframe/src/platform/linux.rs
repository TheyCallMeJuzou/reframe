//! What reframe asks of a Linux desktop itself, beyond what GPUI offers (see `platform`).
//!
//! One reframe per profile: a lock file in the user's runtime folder, held as long as the
//! process lives, and a socket next to it that a second start talks to (Electron's
//! `requestSingleInstanceLock` did the same with its own files). Starting with the system: an
//! XDG autostart entry, as 0.x wrote it.
//!
//! GPUI has no per-window hide on Linux and no system menus or tray: a window's handle is never
//! known here (`hwnd_of` is `None`), so the functions that take one are never called with one.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::Window;

use super::{ContextEntry, TrayEntry, TrayEvent};

// ---------- windows ----------

/// The system's handle of a GPUI window: none that reframe could use on Linux.
pub fn hwnd_of(_window: &Window) -> Option<isize> {
    None
}

pub fn hide(_window: isize) {}

pub fn show(_window: isize, _activate: bool) {}

pub fn is_visible(_window: isize) -> bool {
    false
}

pub fn flash(_window: isize) {}

pub fn client_rect(_window: isize) -> Option<(i32, i32, i32, i32)> {
    None
}

/// GPUI's Linux windows tell of their client area and are placed by it: no frame to add.
pub fn window_frame() -> reframe_core::geometry::WindowFrame {
    reframe_core::geometry::WindowFrame {
        width: 0,
        height: 0,
        scale: 1.0,
    }
}

/// The windows show their menus themselves on Linux (see `window::Shell::show_menu`).
pub fn show_context_menu(_window: isize, _x: i32, _y: i32, _entries: Vec<ContextEntry>) {}

/// No taskbar badges on Linux (0.x had none either).
pub fn set_badge(_window: isize, _description: Option<&str>) {}

// ---------- one reframe per profile ----------

/// The user's runtime folder (`XDG_RUNTIME_DIR`), else the temporary one.
fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
}

fn socket_path(key: &str) -> PathBuf {
    runtime_dir().join(format!("{key}.sock"))
}

/// Whether this is the only reframe for the profile. The claim lasts as long as the process.
pub fn claim_profile(key: &str) -> bool {
    use std::os::fd::IntoRawFd;
    let path = runtime_dir().join(format!("{key}.lock"));
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    else {
        // Without a lock file there is no telling; one reframe more is better than none.
        return true;
    };
    // The descriptor stays open (and locked) until the process ends, on purpose.
    let fd = file.into_raw_fd();
    // SAFETY: a lock on a descriptor this process owns.
    unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

/// Sends one line to the reframe that runs for the profile. False when none answers.
fn tell_running(key: &str, line: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path(key)) else {
        return false;
    };
    stream.write_all(format!("{line}\n").as_bytes()).is_ok()
}

/// Tells the reframe that runs for the profile to show its windows. `build`: which build this
/// one is (see [`TrayEvent::Reveal`]). False when it does not answer (it is still starting, or
/// on its way out).
pub fn reveal_running(key: &str, build: usize) -> bool {
    tell_running(key, &format!("reveal {build}"))
}

/// Asks the reframe that runs for the profile to quit as the user would. False when none runs.
pub fn ask_running_to_quit(key: &str) -> bool {
    tell_running(key, "quit")
}

/// What a line from a second start asks for.
fn request_of(line: &str) -> Option<TrayEvent> {
    let mut words = line.split_whitespace();
    match words.next()? {
        "reveal" => Some(TrayEvent::Reveal {
            build: words.next().and_then(|build| build.parse().ok()).unwrap_or(2),
        }),
        "quit" => Some(TrayEvent::Quit),
        _ => None,
    }
}

/// Listens for a second start (and an updater's request to quit) on the profile's socket; what
/// they ask for arrives at `sender`. Call once the profile is claimed.
fn listen(key: &str, sender: UnboundedSender<TrayEvent>) -> bool {
    let path = socket_path(key);
    // One left by a reframe that did not end properly: the lock says this one is the only one.
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        return false;
    };
    std::thread::Builder::new()
        .name("reframe-second-start".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                if BufReader::new(stream).read_line(&mut line).is_ok()
                    && let Some(event) = request_of(&line)
                    && sender.unbounded_send(event).is_err()
                {
                    break;
                }
            }
        })
        .is_ok()
}

/// A message box with an error, for when reframe cannot start (there is no window yet): the
/// desktop's own dialog tool, where there is one (the caller also writes it to the log and the
/// terminal).
pub fn error_box(title: &str, message: &str) {
    let tools: [(&str, Vec<String>); 2] = [
        (
            "zenity",
            vec![
                "--error".into(),
                format!("--title={title}"),
                format!("--text={message}"),
                "--no-markup".into(),
            ],
        ),
        (
            "kdialog",
            vec!["--title".into(), title.into(), "--error".into(), message.into()],
        ),
    ];
    for (tool, args) in tools {
        if std::process::Command::new(tool).args(&args).status().is_ok() {
            return;
        }
    }
}

// ---------- the tray, and the system going down ----------

struct Control {
    tooltip: String,
    menu: Vec<TrayEntry>,
}

thread_local! {
    static CONTROL: RefCell<Option<Control>> = const { RefCell::new(None) };
}

/// Starts listening for a second start. Its events arrive at `sender`. Returns no handle (there
/// is no window behind it on Linux).
pub fn create_control(key: &str, sender: UnboundedSender<TrayEvent>) -> Option<isize> {
    CONTROL.with_borrow_mut(|control| {
        *control = Some(Control {
            tooltip: String::new(),
            menu: Vec::new(),
        });
    });
    listen(key, sender);
    None
}

/// Nothing to ask for yet (see the tray module's note on the system going down).
pub fn shut_down_first() {}

/// Puts the icon into the tray. Whether there is one.
pub fn add_tray_icon(_small: &[u8], _large: &[u8]) -> bool {
    false
}

/// The tray icon's tooltip and menu, as they are from now on.
pub fn set_tray(tooltip: &str, menu: Vec<TrayEntry>) {
    CONTROL.with_borrow_mut(|control| {
        if let Some(control) = control {
            control.tooltip = tooltip.to_owned();
            control.menu = menu;
        }
    });
}

/// The tray menu and tooltip as they are now (for tests, which cannot open the menu).
pub fn tray() -> (String, Vec<TrayEntry>) {
    CONTROL.with_borrow(|control| {
        control
            .as_ref()
            .map(|control| (control.tooltip.clone(), control.menu.clone()))
            .unwrap_or_default()
    })
}

pub fn remove_tray_icon() {}

// ---------- starting with the system ----------

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn autostart_entry() -> PathBuf {
    reframe_core::autostart::entry_path(std::env::var("XDG_CONFIG_HOME").ok().as_deref(), &home())
}

/// Whether the desktop starts reframe when the user logs in (0.x's entry counts: it is the same
/// file).
pub fn starts_with_system(_app_id: &str) -> bool {
    autostart_entry().is_file()
}

/// Has the desktop start this copy of reframe when the user logs in (in the tray), or no longer.
pub fn set_start_with_system(_app_id: &str, enabled: bool) -> Result<(), String> {
    let entry = autostart_entry();
    if !enabled {
        return match std::fs::remove_file(&entry) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let file = reframe_core::autostart::launch_file(&exe, std::env::var("APPIMAGE").ok().as_deref());
    write_entry(&entry, &file)
}

fn write_entry(entry: &Path, file: &Path) -> Result<(), String> {
    if let Some(folder) = entry.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let text = reframe_core::autostart::desktop_entry(&file.to_string_lossy(), &[crate::app::AUTOSTART_ARG]);
    std::fs::write(entry, text).map_err(|error| error.to_string())
}

/// The user's language, as "de-AT".
pub fn system_locale() -> String {
    let var = |name: &str| std::env::var(name).ok();
    reframe_core::i18n::posix_locale(
        var("LC_ALL").as_deref(),
        var("LC_MESSAGES").as_deref(),
        var("LANG").as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_start_asks_to_reveal_or_to_quit() {
        assert_eq!(request_of("reveal 1\n"), Some(TrayEvent::Reveal { build: 1 }));
        assert_eq!(request_of("reveal"), Some(TrayEvent::Reveal { build: 2 }));
        assert_eq!(request_of("quit\n"), Some(TrayEvent::Quit));
        assert_eq!(request_of("anything else"), None);
        assert_eq!(request_of(""), None);
    }

    #[test]
    fn the_autostart_entry_is_written_with_the_command() {
        let folder = std::env::temp_dir().join(format!("reframe-autostart-{}", std::process::id()));
        let entry = folder.join("autostart").join("reframe.desktop");
        write_entry(&entry, Path::new("/opt/reframe/reframe")).unwrap();
        let text = std::fs::read_to_string(&entry).unwrap();
        assert!(text.contains("Exec=\"/opt/reframe/reframe\" \"--autostart\"\n"));
        let _ = std::fs::remove_dir_all(folder);
    }
}
