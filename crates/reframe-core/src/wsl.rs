//! Folders in WSL distributions as Windows sees them (`\\wsl.localhost\Ubuntu\home\ann\shop`) and as Linux does
//! (`/home/ann/shop`). A session whose folder is one of these runs its claude in that distribution.
//!
//! Ported from the Electron version's `src/shared/wsl.ts`. Paths are plain strings, not `std::path::Path`: they
//! are Windows UNC paths and Linux paths, handled the same on any platform.

/// Where Windows shows the distributions; `\\wsl$` is the older name of the same.
pub const WSL_SHARES: [&str; 2] = ["\\\\wsl.localhost", "\\\\wsl$"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslPath {
    pub distro: String,
    /// The absolute Linux path.
    pub path: String,
}

/// The distribution and Linux path of a folder in WSL, or `None` for any other folder. `share` is where the
/// distributions are (tests use a plain folder); the usual names are always recognized.
///
/// As in the original, an empty `share` counts as none given.
pub fn parse_wsl_path(windows_path: &str, share: Option<&str>) -> Option<WslPath> {
    let normalized = windows_path.replace('/', "\\");
    let own = share.filter(|share| !share.is_empty());
    for root in own.into_iter().chain(WSL_SHARES) {
        let prefix = format!("{}\\", root.trim_end_matches(['\\', '/']));
        let Some((head, tail)) = split_at_utf16(&normalized, prefix.encode_utf16().count()) else {
            continue;
        };
        if head.to_lowercase() != prefix.to_lowercase() {
            continue;
        }
        let mut parts = tail.split('\\').filter(|part| !part.is_empty());
        let distro = parts.next()?;
        return Some(WslPath {
            distro: distro.to_string(),
            path: format!("/{}", parts.collect::<Vec<_>>().join("/")),
        });
    }
    None
}

/// The Windows path of a Linux path in a distribution, under `share`.
pub fn wsl_windows_path(distro: &str, linux_path: &str, share: &str) -> String {
    let rest = linux_path.split('/').filter(|part| !part.is_empty());
    [share.trim_end_matches(['\\', '/']), distro]
        .into_iter()
        .chain(rest)
        .collect::<Vec<_>>()
        .join("\\")
}

/// A Windows path as programs in `distro` reach it: a drive under the mount root (`C:\x` → `/mnt/c/x`), a folder
/// of the distribution itself as its Linux path. `None` for what the distribution cannot reach (another one's
/// folders).
pub fn linux_path_of(windows_path: &str, distro: &str, mount_root: &str, share: Option<&str>) -> Option<String> {
    if let Some((letter, rest)) = drive_path(windows_path) {
        let mut path = format!("{}/{}", mount_root.trim_end_matches('/'), letter.to_ascii_lowercase());
        for part in rest.split(['\\', '/']).filter(|part| !part.is_empty()) {
            path.push('/');
            path.push_str(part);
        }
        return Some(path);
    }
    let own = parse_wsl_path(windows_path, share)?;
    (own.distro.to_lowercase() == distro.to_lowercase()).then_some(own.path)
}

/// The drive letter and what follows it, for a path the original's `/^([A-Za-z]):(?:[\\/](.*))?$/` matches.
/// Its `.` stops at a line break, so a path with one in it is not a drive path.
fn drive_path(windows_path: &str) -> Option<(char, &str)> {
    let mut chars = windows_path.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str();
    if rest.is_empty() {
        return Some((letter, ""));
    }
    let rest = rest.strip_prefix(['\\', '/'])?;
    if rest.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    Some((letter, rest))
}

/// `text` cut after its first `units` UTF-16 code units, which is how the original's `slice` counts: the whole
/// text and nothing after it if it is shorter, `None` if the cut falls inside a character (such a piece never
/// equals a share's name).
fn split_at_utf16(text: &str, units: usize) -> Option<(&str, &str)> {
    let mut seen = 0;
    for (index, character) in text.char_indices() {
        if seen == units {
            return Some(text.split_at(index));
        }
        if seen > units {
            return None;
        }
        seen += character.len_utf16();
    }
    if seen > units { None } else { Some((text, "")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wsl(distro: &str, path: &str) -> Option<WslPath> {
        Some(WslPath {
            distro: distro.to_string(),
            path: path.to_string(),
        })
    }

    // parseWslPath

    #[test]
    fn parse_wsl_path_knows_both_names_of_the_wsl_share_in_any_case_and_with_either_slash() {
        assert_eq!(
            parse_wsl_path("\\\\wsl.localhost\\Ubuntu\\home\\ann\\shop", None),
            wsl("Ubuntu", "/home/ann/shop")
        );
        assert_eq!(parse_wsl_path("\\\\WSL$\\Debian\\srv\\", None), wsl("Debian", "/srv"));
        assert_eq!(parse_wsl_path("//wsl.localhost/Ubuntu", None), wsl("Ubuntu", "/"));
    }

    #[test]
    fn parse_wsl_path_takes_a_share_of_its_own_and_leaves_other_folders_alone() {
        assert_eq!(
            parse_wsl_path("C:\\tmp\\share\\Fake\\home\\ann", Some("C:\\tmp\\share")),
            wsl("Fake", "/home/ann")
        );
        assert_eq!(parse_wsl_path("C:\\Users\\ann\\shop", None), None);
        assert_eq!(parse_wsl_path("\\\\server\\share\\shop", None), None);
        assert_eq!(parse_wsl_path("\\\\wsl.localhost\\", None), None);
        assert_eq!(parse_wsl_path("\\\\wsl.localhostX\\Ubuntu\\home", None), None);
    }

    // wslWindowsPath

    #[test]
    fn wsl_windows_path_puts_a_linux_path_under_the_share() {
        assert_eq!(
            wsl_windows_path("Ubuntu", "/home/ann/shop", "\\\\wsl.localhost"),
            "\\\\wsl.localhost\\Ubuntu\\home\\ann\\shop"
        );
        assert_eq!(
            wsl_windows_path("Ubuntu", "/", "C:\\tmp\\share\\"),
            "C:\\tmp\\share\\Ubuntu"
        );
    }

    // linuxPathOf

    #[test]
    fn linux_path_of_reaches_drives_under_the_mount_root_and_the_distribution_folders_as_linux_paths() {
        assert_eq!(
            linux_path_of("C:\\Users\\Ann\\Pictures\\shot.png", "Ubuntu", "/mnt/", None).as_deref(),
            Some("/mnt/c/Users/Ann/Pictures/shot.png")
        );
        assert_eq!(linux_path_of("D:\\", "Ubuntu", "/mnt", None).as_deref(), Some("/mnt/d"));
        assert_eq!(
            linux_path_of("\\\\wsl.localhost\\Ubuntu\\home\\ann\\a b.txt", "ubuntu", "/mnt/", None).as_deref(),
            Some("/home/ann/a b.txt")
        );
    }

    #[test]
    fn linux_path_of_has_no_path_for_what_the_distribution_cannot_reach() {
        assert_eq!(
            linux_path_of("\\\\wsl.localhost\\Debian\\home\\x", "Ubuntu", "/mnt/", None),
            None
        );
        assert_eq!(linux_path_of("\\\\server\\share\\x", "Ubuntu", "/mnt/", None), None);
    }

    // Not in the original's tests: what the port had to get right beyond them.

    #[test]
    fn the_port_keeps_the_edges_of_the_original() {
        // An empty share is none (`share ? [share] : []`), so it does not match every path.
        assert_eq!(parse_wsl_path("C:\\Users\\ann", Some("")), None);
        // The share's own trailing separators do not count, and its case does not either.
        assert_eq!(
            parse_wsl_path("c:/TMP/share/Fake", Some("C:\\tmp\\share\\/")),
            wsl("Fake", "/")
        );
        // A drive alone, with either slash, and with separators in a row.
        assert_eq!(linux_path_of("c:", "Ubuntu", "/mnt//", None).as_deref(), Some("/mnt/c"));
        assert_eq!(
            linux_path_of("E:/a\\\\b//c", "Ubuntu", "", None).as_deref(),
            Some("/e/a/b/c")
        );
        // Not a drive path: no separator after the colon, or a line break in it.
        assert_eq!(linux_path_of("C:x", "Ubuntu", "/mnt", None), None);
        assert_eq!(linux_path_of("C:\\a\nb", "Ubuntu", "/mnt", None), None);
        // The distribution keeps its spelling, and an empty one is still a path element.
        assert_eq!(wsl_windows_path("", "home//ann/", "\\\\wsl$"), "\\\\wsl$\\\\home\\ann");
    }

    #[test]
    fn the_share_is_compared_in_utf16_units_as_the_original_does() {
        // A path shorter than the share, and one whose cut would fall inside a character.
        assert_eq!(parse_wsl_path("\\\\wsl", None), None);
        assert_eq!(parse_wsl_path("\\\\wsl.localhos\u{1F600}Ubuntu", None), None);
        // Characters outside the BMP after the share are untouched.
        assert_eq!(
            parse_wsl_path("\\\\wsl$\\Ubuntu\\home\\\u{1F600}", None),
            wsl("Ubuntu", "/home/\u{1F600}")
        );
    }
}
