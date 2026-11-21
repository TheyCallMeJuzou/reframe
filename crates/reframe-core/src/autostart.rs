//! Starting reframe with the system on Linux: desktops start what is in `~/.config/autostart`.
//! reframe then starts in the tray, with its windows hidden and every session resumed in the
//! background. (Windows keeps it under its Run key, see the app's `platform` module.)
//!
//! Ported from the Electron version's `src/main/autostart.ts`.

use std::path::{Path, PathBuf};

/// The file to start: the AppImage where it lies (`APPIMAGE`), not the copy of reframe in its
/// temporary mount; else reframe itself.
pub fn launch_file(exe: &Path, app_image: Option<&str>) -> PathBuf {
    match app_image.filter(|image| !image.is_empty()) {
        Some(image) => PathBuf::from(image),
        None => exe.to_path_buf(),
    }
}

/// An XDG autostart entry that runs `file` with `args`, every part quoted (0.x wrote the same).
pub fn desktop_entry(file: &str, args: &[&str]) -> String {
    let quote = |value: &str| {
        let mut quoted = String::from("\"");
        for ch in value.chars() {
            if matches!(ch, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
        quoted.push('"');
        quoted
    };
    let exec: Vec<String> = std::iter::once(file).chain(args.iter().copied()).map(quote).collect();
    [
        "[Desktop Entry]",
        "Type=Application",
        "Name=reframe",
        "Comment=Claude Code sessions side by side",
        &format!("Exec={}", exec.join(" ")),
        "X-GNOME-Autostart-enabled=true",
        "",
    ]
    .join("\n")
}

/// Where the entry goes: `$XDG_CONFIG_HOME/autostart/reframe.desktop`, else under `~/.config`.
pub fn entry_path(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    xdg_config_home
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from)
        .join("autostart")
        .join("reframe.desktop")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_image_starts_from_where_its_file_is_not_from_its_temporary_mount() {
        let exe = Path::new("/tmp/.mount_reframe/reframe");
        assert_eq!(
            launch_file(exe, Some("/home/me/reframe.AppImage")),
            PathBuf::from("/home/me/reframe.AppImage")
        );
        assert_eq!(launch_file(exe, None), exe);
        assert_eq!(launch_file(exe, Some("")), exe);
    }

    #[test]
    fn the_entry_has_a_quoted_command_line() {
        let entry = desktop_entry("/home/me/My Apps/reframe.AppImage", &["--autostart"]);
        assert!(entry.contains("Exec=\"/home/me/My Apps/reframe.AppImage\" \"--autostart\"\n"));
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(entry.ends_with("X-GNOME-Autostart-enabled=true\n"));
        assert!(desktop_entry("/a/\"b\"$c", &[]).contains("Exec=\"/a/\\\"b\\\"\\$c\""));
    }

    #[test]
    fn the_entry_goes_into_the_config_folders_autostart() {
        let home = Path::new("/home/me");
        assert_eq!(
            entry_path(None, home),
            PathBuf::from("/home/me/.config/autostart/reframe.desktop")
        );
        assert_eq!(
            entry_path(Some("/xdg"), home),
            PathBuf::from("/xdg/autostart/reframe.desktop")
        );
        assert_eq!(
            entry_path(Some(""), home),
            PathBuf::from("/home/me/.config/autostart/reframe.desktop")
        );
    }
}
