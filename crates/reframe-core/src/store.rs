//! Loads and saves `state.json`.
//!
//! Ported from the Electron version's `src/main/state/store.ts`.
//!
//! Writes are atomic (temp file + rename) and the previous good file is kept as `state.json.bak`, so a crash
//! mid-write or a corrupt file never loses the layout.
//! A file that cannot be used is never written over: one from a newer reframe is left alone (nothing is written at
//! all, so going back to that version keeps everything), any other is set aside as `state.json.rejected-<time>`.
//!
//! The Electron version debounces frequent saves with a timer of its own (`saveSoon`). This store owns neither a
//! timer nor a thread: the app's event loop does the waiting. It calls [`StateStore::mark_dirty`] when the state
//! changed, and when that returns `true` (the first change since the last write) schedules a call of
//! [`StateStore::flush`] in [`SAVE_DELAY`] ([`StateStore::save_due_at`] is that moment). On shutdown it calls
//! [`StateStore::flush`] once more, which writes synchronously whatever is still waiting.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::state::{AppState, Preferences, STATE_VERSION, SessionRecord, StateError, WindowState, Workspace};

/// How long frequent saves (window moves, resizes) are coalesced into one write: the caller waits this long after
/// the first [`StateStore::mark_dirty`] before it calls [`StateStore::flush`].
pub const SAVE_DELAY: Duration = Duration::from_millis(500);

/// How many set-aside files are kept.
const KEEP_REJECTED: usize = 5;

/// Where the loaded state comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadSource {
    /// `state.json`.
    File,
    /// `state.json.bak`: the file is missing or could not be used.
    Backup,
    /// Neither file could be used: the state of a first start.
    Default,
    /// The file (or, the file being missing or unusable, its backup) belongs to a newer reframe: the state is the
    /// one of a first start, and the store writes nothing from now on.
    Newer,
}

/// The file could not be used and was set aside under this name, for this reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    /// The file's new name. The store's own file, if it could not be moved (held open by something).
    pub file: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadResult {
    pub state: AppState,
    pub source: LoadSource,
    /// The file could not be used and was set aside under this name, for this reason.
    pub rejected: Option<Rejected>,
}

/// Size and time of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Known {
    size: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug)]
pub struct StateStore {
    file: PathBuf,
    /// Since when a save is waiting (the Electron version's timer and pending state).
    dirty_since: Option<Instant>,
    /// Size and time of the file as this store last loaded or wrote it: while they match, it is known to be good.
    known: Option<Known>,
    /// The file belongs to a newer reframe: nothing is written.
    read_only: bool,
}

impl StateStore {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            dirty_since: None,
            known: None,
            read_only: false,
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn backup_file(&self) -> PathBuf {
        with_suffix(&self.file, ".bak")
    }

    /// The file belongs to a newer reframe ([`LoadSource::Newer`]): nothing is written.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Reads the file, or else its backup, or else starts from the defaults. An error is one of setting an unusable
    /// file aside (listing the folder, removing the oldest set-aside files), nothing else.
    pub fn load(&mut self) -> io::Result<LoadResult> {
        let from_backup = match read_file(&self.file) {
            Read::State(state) => {
                self.known = stat(&self.file);
                return Ok(LoadResult {
                    state: *state,
                    source: LoadSource::File,
                    rejected: None,
                });
            }
            Read::Newer => {
                self.read_only = true;
                return Ok(LoadResult {
                    state: AppState::default(),
                    source: LoadSource::Newer,
                    rejected: None,
                });
            }
            Read::Missing => (None, read_file(&self.backup_file())),
            Read::Unreadable(error) => return Err(error),
            Read::Unusable(reason) => {
                let rejected = Rejected {
                    file: self.set_aside()?,
                    reason,
                };
                (Some(rejected), read_file(&self.backup_file()))
            }
        };
        let (rejected, from_backup) = from_backup;
        Ok(match from_backup {
            Read::Newer => {
                // The backup is a newer reframe's as well: it must not be written over either.
                self.read_only = true;
                LoadResult {
                    state: AppState::default(),
                    source: LoadSource::Newer,
                    rejected,
                }
            }
            Read::State(state) => LoadResult {
                state: *state,
                source: LoadSource::Backup,
                rejected,
            },
            Read::Missing | Read::Unusable(_) | Read::Unreadable(_) => LoadResult {
                state: AppState::default(),
                source: LoadSource::Default,
                rejected,
            },
        })
    }

    /// Writes immediately (and cancels a pending debounced write).
    pub fn save_now(&mut self, state: &AppState) -> io::Result<()> {
        self.cancel();
        if self.read_only {
            return Ok(());
        }
        fs::create_dir_all(self.dir())?;
        let temp = with_suffix(&self.file, &format!(".{}.tmp", std::process::id()));
        {
            let mut out = File::create(&temp)?;
            out.write_all(state.to_json().as_bytes())?;
            out.sync_all()?;
        }
        if self.is_good() {
            fs::copy(&self.file, self.backup_file())?;
        }
        rename_with_retry(&temp, &self.file)?;
        self.known = stat(&self.file);
        Ok(())
    }

    /// Coalesces frequent saves (window moves, resizes) into one write: notes that the state changed and is to be
    /// written. `true` if no save was waiting yet: the caller then schedules a [`StateStore::flush`] in
    /// [`SAVE_DELAY`]. Further changes within that time do not move it.
    pub fn mark_dirty(&mut self) -> bool {
        if self.dirty_since.is_some() {
            return false;
        }
        self.dirty_since = Some(Instant::now());
        true
    }

    /// Whether a save is waiting.
    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    /// When the waiting save is to be written: [`SAVE_DELAY`] after the first change.
    pub fn save_due_at(&self) -> Option<Instant> {
        self.dirty_since.map(|since| since + SAVE_DELAY)
    }

    /// Whether a save is waiting and its time has come.
    pub fn is_due(&self, now: Instant) -> bool {
        self.save_due_at().is_some_and(|due| now >= due)
    }

    /// Writes the state if a save is waiting: when its time has come, and once more on shutdown.
    pub fn flush(&mut self, state: &AppState) -> io::Result<()> {
        if self.is_dirty() { self.save_now(state) } else { Ok(()) }
    }

    fn cancel(&mut self) {
        self.dirty_since = None;
    }

    /// The folder the file is in.
    fn dir(&self) -> &Path {
        match self.file.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        }
    }

    /// Whether the file on disk is a good one, worth keeping as the backup; read again only if it changed meanwhile.
    fn is_good(&self) -> bool {
        let Some(now) = stat(&self.file) else {
            return false;
        };
        if self.known == Some(now) {
            return true;
        }
        matches!(read_file(&self.file), Read::State(_))
    }

    /// Moves the unusable file out of the way (beyond KEEP_REJECTED such files, the oldest go); its new name.
    fn set_aside(&self) -> io::Result<PathBuf> {
        let dir = self.dir();
        let base = self.file.file_name().map(|name| name.to_string_lossy().into_owned());
        let prefix = format!("{}.rejected-", base.unwrap_or_default());
        let stamp = utc_stamp(SystemTime::now());
        // Within the same second, numbered on from the highest number there (never one freed by trimming), so that
        // the names sort by age.
        let start = format!("{prefix}{stamp}");
        let same: Vec<String> = names_in(dir)?
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| name.starts_with(&start))
            .collect();
        let highest = same.iter().map(|name| number_of(name).unwrap_or(1)).max().unwrap_or(0);
        let number = if same.is_empty() {
            String::new()
        } else {
            format!("-{:03}", highest + 1)
        };
        let target = dir.join(format!("{start}{number}"));
        if fs::rename(&self.file, &target).is_err() {
            // Held open by something: it stays where it is, and the next save will not take it for the backup either.
            return Ok(self.file.clone());
        }
        let mut old: Vec<(String, OsString)> = names_in(dir)?
            .into_iter()
            .filter(|(name, _)| name.starts_with(&prefix))
            .collect();
        // In JavaScript's order (by UTF-16 code unit), as the Electron version sorts them.
        old.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
        old.truncate(old.len().saturating_sub(KEEP_REJECTED));
        for (_, name) in old {
            match fs::remove_file(dir.join(name)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(target)
    }
}

/// The path with something appended to its last part.
fn with_suffix(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The names in a folder: as text, and as they are.
fn names_in(dir: &Path) -> io::Result<Vec<(String, OsString)>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name();
        names.push((name.to_string_lossy().into_owned(), name));
    }
    Ok(names)
}

/// The number a set-aside file's name ends in (`-NNN`), if it has one.
fn number_of(name: &str) -> Option<u32> {
    let bytes = name.as_bytes();
    let tail = bytes.len().checked_sub(4).map(|at| &bytes[at..])?;
    if tail[0] != b'-' || !tail[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(
        tail[1..]
            .iter()
            .fold(0, |number, digit| number * 10 + u32::from(digit - b'0')),
    )
}

/// The time as `YYYYMMDD-HHMMSS` in UTC (what the Electron version cuts out of `Date.toISOString()`).
fn utc_stamp(time: SystemTime) -> String {
    // A clock before 1970 counts as 1970.
    let seconds = time.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // The date of a day number, in the calendar's 400-year cycles that start on a 1st of March.
    let shifted = days + 719_468;
    let (era, day_of_era) = (shifted / 146_097, shifted % 146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The state in a file, or why there is none: missing (no reason), from a newer reframe, or unusable.
enum Read {
    State(Box<AppState>),
    Missing,
    Newer,
    Unusable(String),
    /// There, but it cannot be read.
    Unreadable(io::Error),
}

fn read_file(file: &Path) -> Read {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Read::Missing,
        // There, but it cannot be read (a folder, no access): as the Electron version, which then
        // could not start, reframe does not run without its state.
        Err(error) => return Read::Unreadable(error),
    };
    // As Node reads it: bytes that are not UTF-8 become replacement characters (and then are not JSON).
    let text = String::from_utf8_lossy(&bytes);
    let json: Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(error) => return Read::Unusable(format!("not JSON ({error})")),
    };
    // Any number above this format's version, also one that is not whole.
    let version = json.get("version").and_then(Value::as_f64);
    if version.is_some_and(|version| version > f64::from(STATE_VERSION)) {
        return Read::Newer;
    }
    match AppState::parse(&text) {
        Ok(state) => Read::State(Box::new(state)),
        Err(StateError::Newer(_)) => Read::Newer,
        Err(StateError::Invalid(reason)) => Read::Unusable(issues(&json, reason)),
    }
}

/// What is wrong with a file that is JSON but not a state, as `<key>: <what>` for up to five of its keys, or
/// `(file): <what>` where no single key is to blame. (The Electron version lists its schema's issues this way; the
/// wording of each is the schema's there and serde's here.)
fn issues(json: &Value, reason: String) -> String {
    fn issue<T: DeserializeOwned>(json: &Value, key: &str) -> Option<String> {
        let error = serde_json::from_value::<T>(json.get(key)?.clone()).err()?;
        Some(format!("{key}: {error}"))
    }
    let found: Vec<String> = [
        issue::<u32>(json, "version"),
        issue::<IndexMap<String, SessionRecord>>(json, "sessions"),
        issue::<Vec<WindowState>>(json, "windows"),
        issue::<Vec<String>>(json, "recentFolders"),
        issue::<Preferences>(json, "preferences"),
        issue::<Vec<Workspace>>(json, "workspaces"),
        issue::<Option<Vec<String>>>(json, "wslDistros"),
    ]
    .into_iter()
    .flatten()
    .take(5)
    .collect();
    if found.is_empty() {
        format!("(file): {reason}")
    } else {
        found.join("; ")
    }
}

fn stat(file: &Path) -> Option<Known> {
    let metadata = fs::metadata(file).ok()?;
    Some(Known {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

// On Windows a scanner or indexer can hold the target open for a moment.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt: u32 = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if attempt >= 4 || !is_sharing_error(&error) => return Err(error),
            Err(_) => {}
        }
        // short synchronous back-off; saves must not interleave
        std::thread::sleep(Duration::from_millis(20 * u64::from(attempt + 1)));
        attempt += 1;
    }
}

/// What Node reports as EPERM, EBUSY or EACCES.
fn is_sharing_error(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy
    ) {
        return true;
    }
    // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
