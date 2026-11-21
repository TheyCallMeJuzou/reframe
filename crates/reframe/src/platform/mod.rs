//! What reframe asks of the system itself, beyond what GPUI offers: windows hidden and shown,
//! the tray icon with its menu, one reframe per profile, being told when the system goes down,
//! and starting with the system. `win32.rs` and `linux.rs` do it for their system, with the same
//! functions; what both share is here.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod win32;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(windows)]
pub use win32::*;

/// What reaches reframe from the system: the tray, a second start, the system going down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// A click on the tray icon.
    Clicked,
    /// An entry of the tray menu was picked.
    Command(u32),
    /// reframe was started a second time: the windows are shown. `build` is 1 for a development
    /// build and 2 for an installed one.
    Reveal { build: usize },
    /// The system shuts down, restarts or logs the user off.
    SystemEnding,
    /// The installer (or updater) asks reframe to quit.
    Quit,
    /// A window's context menu was answered: the number of the entry picked, or none (Windows,
    /// where the menus are the system's).
    #[cfg(windows)]
    ContextMenu { window: isize, picked: Option<u32> },
}

/// An entry of the tray menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEntry {
    Item {
        id: u32,
        label: String,
        /// With a check mark or without; `None` for an entry that has none.
        checked: Option<bool>,
        enabled: bool,
    },
    Separator,
    /// An entry that opens more entries.
    Submenu {
        label: String,
        children: Vec<TrayEntry>,
    },
}

/// An entry of a window's context menu (a tile's, a group's, the session list's), as the
/// system shows it: 0.x's menus were the system's too. (On Linux the windows draw their menus.)
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct ContextEntry {
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    /// A dot in this colour (red, green, blue) before the label: a group's.
    pub swatch: Option<(u8, u8, u8)>,
    /// A line above it.
    pub separator: bool,
    /// What comes back when it is picked (not 0); a submenu's is not used.
    pub id: u32,
    pub children: Vec<ContextEntry>,
}

/// What other programs' windows do, as far as terminal windows dragged onto reframe go.
#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// A window's move (or resize) by its title bar began or ended.
    MoveStarted(isize),
    MoveEnded(isize),
    /// A top-level window was made, or shown.
    Created(isize),
    Shown(isize),
}

/// A name for the profile that the system takes in names of its own: no slashes. Windows
/// compares paths without case, so the key does too there.
pub fn profile_key(user_data: &std::path::Path) -> String {
    let path = user_data.to_string_lossy();
    let path = if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.into_owned()
    };
    // FNV-1a over the path as the system compares it.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("reframe-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::profile_key;

    #[cfg(windows)]
    #[test]
    fn a_profile_has_one_key_however_its_path_is_written() {
        let key = profile_key(std::path::Path::new("C:\\Users\\Ann\\AppData\\Roaming\\reframe"));
        assert_eq!(
            key,
            profile_key(std::path::Path::new("c:\\users\\ann\\appdata\\roaming\\REFRAME"))
        );
        assert_ne!(key, profile_key(std::path::Path::new("D:\\portable\\data")));
        assert!(key.starts_with("reframe-") && !key.contains('\\'));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_profile_key_tells_paths_apart_by_case() {
        let key = profile_key(std::path::Path::new("/home/ann/.config/reframe"));
        assert_ne!(key, profile_key(std::path::Path::new("/home/ann/.config/Reframe")));
        assert_eq!(key, profile_key(std::path::Path::new("/home/ann/.config/reframe")));
        assert!(key.starts_with("reframe-") && !key.contains('/'));
    }
}
