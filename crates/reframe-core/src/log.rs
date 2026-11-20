//! Plain-text log in the profile (logs/main.log), for finding out afterwards what happened.
//! One line per event; the file is rotated to main.old.log once it passes 2 MB.
//!
//! Ported from the Electron version's `src/main/log.ts`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    /// The level as the lines have it.
    fn name(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// Writes the log. Cheap to clone; all clones write the same file, one line at a time.
#[derive(Clone, Debug)]
pub struct Logger {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    dir: PathBuf,
    file: PathBuf,
    /// Whether the folder is known to be there. Holding it is what keeps the lines (and the rotation) apart.
    checked: Mutex<bool>,
}

impl Logger {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let file = dir.join("main.log");
        Self {
            inner: Arc::new(Inner {
                dir,
                file,
                checked: Mutex::new(false),
            }),
        }
    }

    pub fn file(&self) -> &Path {
        &self.inner.file
    }

    pub fn log(&self, level: Level, message: &str, data: Option<Value>) {
        let data = data.map(|data| format!(" {data}")).unwrap_or_default();
        let line = format!(
            "{} {:<5} {message}{data}\n",
            iso_string(SystemTime::now()),
            level.name()
        );
        {
            let mut checked = self.inner.checked.lock().unwrap_or_else(PoisonError::into_inner);
            // logging must never break the app
            let _ = self.inner.append(&mut checked, &line);
        }
        let everything = std::env::var_os("REFRAME_LOG_STDOUT").is_some_and(|value| !value.is_empty());
        if level != Level::Info || everything {
            let _ = io::stderr().write_all(line.as_bytes());
        }
    }

    pub fn info(&self, message: &str, data: Option<Value>) {
        self.log(Level::Info, message, data);
    }

    pub fn warn(&self, message: &str, data: Option<Value>) {
        self.log(Level::Warn, message, data);
    }

    pub fn error(&self, message: &str, data: Option<Value>) {
        self.log(Level::Error, message, data);
    }
}

impl Inner {
    fn append(&self, checked: &mut bool, line: &str) -> io::Result<()> {
        if !*checked {
            fs::create_dir_all(&self.dir)?;
            *checked = true;
        }
        if fs::metadata(&self.file).is_ok_and(|file| file.len() > MAX_BYTES) {
            fs::rename(&self.file, self.dir.join("main.old.log"))?;
        }
        let mut file = OpenOptions::new().append(true).create(true).open(&self.file)?;
        file.write_all(line.as_bytes())
    }
}

/// The time as JavaScript's `Date.toISOString()` gives it: `YYYY-MM-DDTHH:MM:SS.mmmZ`, in UTC.
fn iso_string(time: SystemTime) -> String {
    // A clock before 1970 counts as 1970.
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (seconds, millis) = (since.as_secs(), since.subsec_millis());
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
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
#[path = "log_tests.rs"]
mod tests;
