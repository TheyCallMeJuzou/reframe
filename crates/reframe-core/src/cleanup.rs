//! What reframe leaves on disk and cleans up after a while: files dropped or pasted into sessions that had no path
//! of their own (saved to the temp folder so a session gets a path for them; every image pasted into a WSL session
//! is one), and the hook settings of sessions that are gone.
//!
//! Ported from the Electron version's `src/main/cleanup.ts`. There the clean-up is asynchronous; here it runs to
//! its end, on whichever thread calls it.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where dropped and pasted files are saved, one folder each.
pub fn drops_dir() -> PathBuf {
    std::env::temp_dir().join("reframe-drops")
}

/// How long a dropped file stays: Claude Code reads it when the prompt is sent, but the prompt may wait.
pub const DROPS_KEEP_MS: f64 = 7.0 * 24.0 * 60.0 * 60_000.0;

/// A name in a folder and when it was last changed, in milliseconds since 1970 (JavaScript's `mtimeMs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub mtime_ms: f64,
}

/// How many of each were removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub drops: usize,
    pub hooks: usize,
}

/// The entries older than `max_age_ms`.
pub fn older_than(entries: &[Entry], now: f64, max_age_ms: f64) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| now - entry.mtime_ms > max_age_ms)
        .map(|entry| entry.name.clone())
        .collect()
}

/// Hook settings files (`<session id>.json`) of sessions reframe no longer has.
pub fn stray_hook_files(names: &[String], session_ids: &HashSet<String>) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.strip_suffix(".json").is_some_and(|id| !session_ids.contains(id)))
        .cloned()
        .collect()
}

/// Removes old drops and stray hook files; how many of each. One that cannot go now (in use) goes next time.
pub fn clean_up(drops: &Path, hooks: &Path, session_ids: &HashSet<String>) -> Removed {
    clean_up_at(drops, hooks, session_ids, millis(SystemTime::now()))
}

/// [`clean_up`] at another time than now (`now` in milliseconds since 1970).
pub fn clean_up_at(drops: &Path, hooks: &Path, session_ids: &HashSet<String>, now: f64) -> Removed {
    let entries = |dir: &Path| -> Vec<Entry> {
        names_in(dir)
            .into_iter()
            .filter_map(|name| {
                let modified = fs::metadata(dir.join(&name)).and_then(|file| file.modified()).ok()?;
                Some(Entry {
                    name,
                    mtime_ms: millis(modified),
                })
            })
            .collect()
    };
    let remove =
        |dir: &Path, names: Vec<String>| names.iter().filter(|name| remove_all(&dir.join(name)).is_ok()).count();
    Removed {
        drops: remove(drops, older_than(&entries(drops), now, DROPS_KEEP_MS)),
        hooks: remove(hooks, stray_hook_files(&names_in(hooks), session_ids)),
    }
}

/// The names in a folder; none if it cannot be read (it is not there).
fn names_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    // As soon as one name cannot be read, Node gives up on the folder as a whole.
    let names: io::Result<Vec<String>> = entries
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect();
    names.unwrap_or_default()
}

/// A time in milliseconds since 1970, with its fraction.
fn millis(time: SystemTime) -> f64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_secs_f64() * 1_000.0,
        Err(before) => -before.duration().as_secs_f64() * 1_000.0,
    }
}

/// Removes a file, or a folder with all that is in it (Node's `rm` with `recursive` and `force`): one that is
/// not there is no error.
fn remove_all(path: &Path) -> io::Result<()> {
    let removed = match fs::symlink_metadata(path) {
        Ok(file) if file.is_dir() => fs::remove_dir_all(path),
        // A link to a folder is a folder of its own kind on Windows, and goes as one (the link only).
        Ok(_) => fs::remove_file(path).or_else(|_| fs::remove_dir_all(path)),
        Err(error) => Err(error),
    };
    match removed {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
