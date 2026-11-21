//! What reframe types into a session for files, the way a terminal would.
//!
//! Ported from the Electron version's `src/renderer/paste.ts`. Its `imagePasteKey` and
//! `pastesImageAsFile` are not here: on Windows, the only platform so far, an image is always
//! saved to a file and its path typed.

use crate::wsl::linux_path_of;

/// Whose rules a path is quoted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quoting {
    /// cmd and PowerShell.
    Windows,
    /// A POSIX shell.
    Posix,
}

fn quote(path: &str, quoting: Quoting) -> String {
    match quoting {
        Quoting::Windows => {
            if path.contains(|c: char| c.is_whitespace() || "&()^'\"`,;=!%".contains(c)) {
                format!("\"{path}\"")
            } else {
                path.to_owned()
            }
        }
        Quoting::Posix => {
            let plain = !path.is_empty()
                && path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
            if plain {
                path.to_owned()
            } else {
                format!("'{}'", path.replace('\'', "'\\''"))
            }
        }
    }
}

/// File paths as one would type them: quoted where needed, separated by spaces, with a space
/// after.
pub fn format_paths<S: AsRef<str>>(paths: &[S], quoting: Quoting) -> String {
    paths.iter().map(|path| quote(path.as_ref(), quoting) + " ").collect()
}

/// For a session in WSL: the files' Linux paths (`C:\x` is `/mnt/c/x`), quoted for its shell.
pub fn format_paths_for_wsl<S: AsRef<str>>(paths: &[S], distro: &str, mount_root: &str) -> String {
    let linux: Vec<String> = paths
        .iter()
        .map(|path| linux_path_of(path.as_ref(), distro, mount_root, None).unwrap_or_else(|| path.as_ref().to_owned()))
        .collect();
    format_paths(&linux, Quoting::Posix)
}

/// Local paths from a list of `file://` addresses, one per line, as Linux file managers put copied
/// files into the clipboard (GNOME's list starts with "copy" or "cut"). From the Electron
/// version's `src/main/clipboard-formats.ts` (`fileUrls`); an address of another machine is no
/// local path (Node's `fileURLToPath` refuses it).
pub fn file_urls(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter_map(|line| {
            let rest = line.strip_prefix("file://")?;
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            if !path.starts_with('/') {
                return None;
            }
            crate::hooks::decode_uri_component(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_files_are_their_local_paths() {
        let gnome = "copy\nfile:///home/me/My%20Files/a.txt\nfile:///tmp/%C3%BC.png\n";
        assert_eq!(file_urls(gnome), ["/home/me/My Files/a.txt", "/tmp/\u{fc}.png"]);
        assert_eq!(file_urls("file://localhost/etc/hosts\r\n"), ["/etc/hosts"]);
        assert!(file_urls("file://server/share/a\nhttps://example.com/\nplain text").is_empty());
    }

    #[test]
    fn quotes_windows_paths_only_where_cmd_and_powershell_would_need_it() {
        assert_eq!(
            format_paths(&["C:\\shots\\a.png", "C:\\My Pictures\\b.png"], Quoting::Windows),
            "C:\\shots\\a.png \"C:\\My Pictures\\b.png\" "
        );
    }

    #[test]
    fn quotes_other_paths_for_a_posix_shell() {
        assert_eq!(
            format_paths(&["/home/me/a.png", "/home/me/it's here.png"], Quoting::Posix),
            "/home/me/a.png '/home/me/it'\\''s here.png' "
        );
    }

    #[test]
    fn types_linux_paths_into_a_session_in_wsl() {
        assert_eq!(
            format_paths_for_wsl(
                &["C:\\My Pictures\\b.png", "\\\\wsl.localhost\\Ubuntu\\home\\me\\a.txt"],
                "Ubuntu",
                "/mnt/"
            ),
            "'/mnt/c/My Pictures/b.png' /home/me/a.txt "
        );
    }

    #[test]
    fn types_nothing_for_no_files() {
        assert_eq!(format_paths::<&str>(&[], Quoting::Windows), "");
    }
}
