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

/// A reframe 0.x (Electron) that runs on this profile, by its process id: Electron's lock is a
/// link `SingletonLock` to "<host>-<pid>" in the profile. Both would manage the same state file.
pub fn older_reframe_running(user_data: &Path) -> Option<u32> {
    let target = std::fs::read_link(user_data.join("SingletonLock")).ok()?;
    let (host, pid) = target.to_str()?.rsplit_once('-')?;
    let pid: u32 = pid.parse().ok()?;
    let here = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    let older =
        host == here.trim() && pid != std::process::id() && crate::process::is_alive(pid) && name.trim() == "reframe";
    older.then_some(pid)
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

/// The tray icon as the desktop's StatusNotifierItem host shows it (as Electron's `Tray` did on
/// Linux; GNOME needs its AppIndicator extension for it, which Ubuntu has on). Its menu and
/// tooltip live here, its clicks go to reframe's events.
struct TrayIcon {
    tooltip: String,
    menu: Vec<TrayEntry>,
    pixmaps: Vec<ksni::Icon>,
    sender: UnboundedSender<TrayEvent>,
    /// Whether a host shows it right now.
    shown: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl TrayIcon {
    fn send(&self, event: TrayEvent) {
        let _ = self.sender.unbounded_send(event);
    }
}

/// A label as a D-Bus menu shows it: an underscore marks the access key there, so a plain one
/// is doubled.
fn menu_label(label: &str) -> String {
    label.replace('_', "__")
}

fn menu_items(entries: &[TrayEntry]) -> Vec<ksni::MenuItem<TrayIcon>> {
    use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
    entries
        .iter()
        .map(|entry| match entry {
            TrayEntry::Separator => ksni::MenuItem::Separator,
            TrayEntry::Item {
                id,
                label,
                checked: Some(checked),
                enabled,
            } => {
                let id = *id;
                CheckmarkItem {
                    label: menu_label(label),
                    enabled: *enabled,
                    checked: *checked,
                    activate: Box::new(move |tray: &mut TrayIcon| tray.send(TrayEvent::Command(id))),
                    ..Default::default()
                }
                .into()
            }
            TrayEntry::Item {
                id,
                label,
                checked: None,
                enabled,
            } => {
                let id = *id;
                StandardItem {
                    label: menu_label(label),
                    enabled: *enabled,
                    activate: Box::new(move |tray: &mut TrayIcon| tray.send(TrayEvent::Command(id))),
                    ..Default::default()
                }
                .into()
            }
            TrayEntry::Submenu { label, children } => SubMenu {
                label: menu_label(label),
                submenu: menu_items(children),
                ..Default::default()
            }
            .into(),
        })
        .collect()
}

impl ksni::Tray for TrayIcon {
    fn id(&self) -> String {
        "reframe".into()
    }

    fn title(&self) -> String {
        "reframe".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.pixmaps.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tooltip.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Clicked);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        menu_items(&self.menu)
    }

    fn watcher_online(&self) {
        self.shown.store(true, std::sync::atomic::Ordering::Release);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // It comes back when a host does (a desktop's tray restarted, an extension turned on).
        self.shown.store(false, std::sync::atomic::Ordering::Release);
        true
    }
}

/// A PNG file as a StatusNotifierItem takes an icon: ARGB, a byte each, big-endian.
fn pixmap(png: &[u8]) -> Option<ksni::Icon> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let data = image
        .pixels()
        .flat_map(|pixel| {
            let [red, green, blue, alpha] = pixel.0;
            [alpha, red, green, blue]
        })
        .collect();
    Some(ksni::Icon {
        width: i32::try_from(image.width()).ok()?,
        height: i32::try_from(image.height()).ok()?,
        data,
    })
}

struct Control {
    tooltip: String,
    menu: Vec<TrayEntry>,
    sender: UnboundedSender<TrayEvent>,
    icon: Option<ksni::blocking::Handle<TrayIcon>>,
    /// Whether a tray shows the icon (see [`has_tray`]).
    shown: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

thread_local! {
    static CONTROL: RefCell<Option<Control>> = const { RefCell::new(None) };
}

/// Starts listening for a second start, and for the system going down. Their events arrive at
/// `sender`. Returns no handle (there is no window behind it on Linux).
pub fn create_control(key: &str, sender: UnboundedSender<TrayEvent>) -> Option<isize> {
    listen(key, sender.clone());
    CONTROL.with_borrow_mut(|control| {
        *control = Some(Control {
            tooltip: String::new(),
            menu: Vec::new(),
            sender,
            icon: None,
            shown: Default::default(),
        });
    });
    None
}

/// reframe is told when the system goes down (logind's `PrepareForShutdown`, which it delays a
/// moment for), and quits as the user would on SIGTERM and SIGINT (a logout ends it so).
pub fn shut_down_first() {
    let Some(sender) = CONTROL.with_borrow(|control| control.as_ref().map(|control| control.sender.clone())) else {
        return;
    };
    let quit = sender.clone();
    let _ = std::thread::Builder::new()
        .name("reframe-signals".into())
        .spawn(move || {
            let Ok(mut signals) =
                signal_hook::iterator::Signals::new([signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT])
            else {
                return;
            };
            if signals.forever().next().is_some() {
                let _ = quit.unbounded_send(TrayEvent::Quit);
            }
        });
    let _ = std::thread::Builder::new()
        .name("reframe-logind".into())
        .spawn(move || {
            let _ = watch_shutdown(&sender);
        });
}

/// Follows logind: while reframe runs it holds a delay lock, so that the system waits for it
/// (logind's `InhibitDelayMaxSec`, 5 seconds unless set) when it goes down.
fn watch_shutdown(sender: &UnboundedSender<TrayEvent>) -> zbus::Result<()> {
    let connection = zbus::blocking::Connection::system()?;
    let manager = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let inhibit = || -> Option<zbus::zvariant::OwnedFd> {
        manager
            .call(
                "Inhibit",
                &("shutdown", "reframe", "Remembering which sessions to resume", "delay"),
            )
            .ok()
    };
    let mut lock = inhibit();
    for message in manager.receive_signal("PrepareForShutdown")? {
        let going_down: bool = message.body().deserialize()?;
        if going_down {
            let _ = sender.unbounded_send(TrayEvent::SystemEnding);
            // A moment to note it, then the system may go on.
            std::thread::sleep(std::time::Duration::from_millis(1000));
            lock = None;
        } else if lock.is_none() {
            lock = inhibit();
        }
    }
    drop(lock);
    Ok(())
}

/// Puts the icon into the tray (`small` and `large` are PNG files of 16 and 32 pixels). Whether
/// a tray shows it; the icon comes as soon as one does.
pub fn add_tray_icon(small: &[u8], large: &[u8]) -> bool {
    use ksni::blocking::TrayMethods;
    CONTROL.with_borrow_mut(|control| {
        let Some(control) = control else { return false };
        let shown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let icon = TrayIcon {
            tooltip: control.tooltip.clone(),
            menu: control.menu.clone(),
            pixmaps: [small, large].into_iter().filter_map(pixmap).collect(),
            sender: control.sender.clone(),
            shown: shown.clone(),
        };
        // Without a host it waits for one instead of failing.
        match icon.assume_sni_available(true).spawn() {
            Ok(handle) => {
                control.icon = Some(handle);
                control.shown = shown;
                control.shown.load(std::sync::atomic::Ordering::Acquire)
            }
            Err(_) => false,
        }
    })
}

/// Whether a tray shows reframe's icon: without one, a window that is closed but kept is
/// minimized instead of hidden (it could not be found again).
pub fn has_tray() -> bool {
    CONTROL.with_borrow(|control| {
        control
            .as_ref()
            .is_some_and(|control| control.icon.is_some() && control.shown.load(std::sync::atomic::Ordering::Acquire))
    })
}

/// The tray icon's tooltip and menu, as they are from now on.
pub fn set_tray(tooltip: &str, menu: Vec<TrayEntry>) {
    CONTROL.with_borrow_mut(|control| {
        let Some(control) = control else { return };
        if control.tooltip == tooltip && control.menu == menu {
            return;
        }
        control.tooltip = tooltip.to_owned();
        control.menu = menu;
        if let Some(icon) = &control.icon {
            let (tooltip, menu) = (control.tooltip.clone(), control.menu.clone());
            icon.update(move |icon| {
                icon.tooltip = tooltip;
                icon.menu = menu;
            });
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

/// Takes the icon out of the tray, and the profile's socket away, when reframe quits.
pub fn remove_tray_icon() {
    CONTROL.with_borrow_mut(|control| {
        if let Some(icon) = control.as_mut().and_then(|control| control.icon.take()) {
            icon.shutdown().wait();
        }
    });
}

// ---------- the clipboard ----------

/// Runs a clipboard tool and gives its output, if it answers within a second (one whose owner
/// hangs must not hang reframe).
fn clipboard_tool(program: &str, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        text
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then(|| reader.join().ok()).flatten(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// Files copied in a file manager: GPUI reads no file lists from Linux's clipboard, so they are
/// asked of `wl-paste` (Wayland) or `xclip` (X11), where those are installed (0.x read the same
/// two formats).
pub fn clipboard_files() -> Vec<PathBuf> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty());
    for format in ["x-special/gnome-copied-files", "text/uri-list"] {
        let text = if wayland {
            clipboard_tool("wl-paste", &["--no-newline", "--type", format])
        } else {
            clipboard_tool("xclip", &["-selection", "clipboard", "-o", "-t", format])
        };
        let files = reframe_core::paste::file_urls(&text.unwrap_or_default());
        if !files.is_empty() {
            return files.into_iter().map(PathBuf::from).collect();
        }
    }
    Vec::new()
}

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
