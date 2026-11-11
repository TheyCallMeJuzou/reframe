//! Folders as people read them.
//!
//! Ported from the Electron version's `src/main/paths.ts`.

/// A folder under the home folder as "~" plus the rest (e.g. "~\projects\shop"), any other as it is.
pub fn home_relative(folder: &str, home: &str, windows: bool) -> String {
    let base = home.trim_end_matches(['\\', '/']);
    if base.is_empty() {
        return folder.to_owned();
    }
    let same = |a: &str, b: &str| {
        if windows {
            a.to_lowercase() == b.to_lowercase()
        } else {
            a == b
        }
    };
    if same(folder, base) {
        return "~".to_owned();
    }
    // The Electron version counts in UTF-16 code units (the home folder's length need not be the length of the
    // same folder in other letter case), so this does too.
    let units: Vec<u16> = folder.encode_utf16().collect();
    let length = base.encode_utf16().count();
    let next = units.get(length).copied();
    let is_separator = next == Some(u16::from(b'/')) || (windows && next == Some(u16::from(b'\\')));
    if !is_separator {
        return folder.to_owned();
    }
    // A start that ends in the middle of a character is not the home folder.
    match String::from_utf16(&units[..length]) {
        Ok(start) if same(&start, base) => format!("~{}", &folder[start.len()..]),
        _ => folder.to_owned(),
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
