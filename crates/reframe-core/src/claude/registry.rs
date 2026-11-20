//! Reads Claude Code's per-process session registry (`~/.claude/sessions/<pid>.json`) and
//! transcripts. The registry format is undocumented, so everything here is parsed defensively
//! and kept in one place.
//!
//! Ported from the Electron version's `src/main/claude/registry.ts`. That file has every reader
//! twice, blocking and not: a WSL distribution's Claude Code folder is behind a network share,
//! which can take seconds to answer (a distribution that stopped is started by it). Here there
//! is one of each, and the caller runs it off the UI thread when the folder is behind a share.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryStatus {
    Idle,
    Busy,
    Waiting,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegistryEntry {
    pub pid: u32,
    pub session_id: String,
    pub cwd: Option<String>,
    pub name: Option<String>,
    /// Whether the user named the session ("user"), or Claude did ("auto", "derived").
    pub name_source: Option<String>,
    pub status: Option<RegistryStatus>,
    /// When the process started (ms since 1970).
    pub started_at: Option<f64>,
    /// The process's start as the system tells it (on Windows its creation time).
    pub proc_start: Option<String>,
}

/// Claude Code's config dir: `CLAUDE_CONFIG_DIR` if set, else `~/.claude`. `REFRAME_CLAUDE_HOME`
/// wins for tests.
pub fn claude_home(env: &HashMap<String, String>, home: &Path) -> PathBuf {
    let set = |name: &str| env.get(name).filter(|value| !value.is_empty());
    match set("REFRAME_CLAUDE_HOME").or_else(|| set("CLAUDE_CONFIG_DIR")) {
        Some(dir) => PathBuf::from(dir),
        None => home.join(".claude"),
    }
}

pub fn parse_registry_entry(value: &Value) -> Option<RegistryEntry> {
    let object = value.as_object()?;
    let text = |key: &str| object.get(key).and_then(Value::as_str);
    let filled = |key: &str| text(key).filter(|value| !value.is_empty()).map(str::to_owned);
    let pid = object.get("pid")?.as_f64()?;
    if pid.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&pid) {
        return None;
    }
    Some(RegistryEntry {
        pid: pid as u32,
        session_id: filled("sessionId")?,
        cwd: text("cwd").map(str::to_owned),
        name: filled("name"),
        name_source: text("nameSource").map(str::to_owned),
        status: match text("status") {
            Some("idle") => Some(RegistryStatus::Idle),
            Some("busy") => Some(RegistryStatus::Busy),
            Some("waiting") => Some(RegistryStatus::Waiting),
            _ => None,
        },
        started_at: object.get("startedAt").and_then(Value::as_f64),
        proc_start: filled("procStart"),
    })
}

/// Whether the claude an entry names runs. A process that was killed leaves its entry behind,
/// and Windows soon gives its id to another program: where the system tells when a process
/// started, that must be when this one did.
///
/// `is_alive` tells whether a process id is in use; `start_of` when that process started, in the
/// registry's own form, or `None` where the system does not tell.
pub fn is_running_entry(
    entry: &RegistryEntry,
    is_alive: &dyn Fn(u32) -> bool,
    start_of: &dyn Fn(u32) -> Option<String>,
) -> bool {
    if !is_alive(entry.pid) {
        return false;
    }
    match (&entry.proc_start, start_of(entry.pid)) {
        (Some(recorded), Some(actual)) => *recorded == actual,
        _ => true,
    }
}

fn entry_file(home: &Path, pid: u32) -> PathBuf {
    home.join("sessions").join(format!("{pid}.json"))
}

/// Removes the entry of a claude of reframe's that was killed (and could not remove it itself),
/// once it is gone.
pub fn forget_registry_entry(home: &Path, pid: u32, is_alive: &dyn Fn(u32) -> bool) {
    if is_alive(pid) || read_registry_entry(home, pid).is_none() {
        return;
    }
    let _ = std::fs::remove_file(entry_file(home, pid));
}

pub fn read_registry_entry(home: &Path, pid: u32) -> Option<RegistryEntry> {
    let text = std::fs::read_to_string(entry_file(home, pid)).ok()?;
    let entry = parse_registry_entry(&serde_json::from_str(&text).ok()?)?;
    (entry.pid == pid).then_some(entry)
}

/// The entry of a running claude by its session id, for one whose process id reframe does not
/// know (in WSL, the entry is named after the Linux process id). `alive` tells a running claude's
/// entry from one a killed claude left behind, which would otherwise be taken for the session's
/// for good.
pub fn find_registry_entry(
    home: &Path,
    session_id: &str,
    alive: Option<&dyn Fn(&RegistryEntry) -> bool>,
) -> Option<RegistryEntry> {
    for file in std::fs::read_dir(home.join("sessions")).ok()?.flatten() {
        let name = file.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
            continue;
        };
        if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Some(entry) = pid.parse().ok().and_then(|pid| read_registry_entry(home, pid)) else {
            continue;
        };
        if entry.session_id == session_id && alive.is_none_or(|alive| alive(&entry)) {
            return Some(entry);
        }
    }
    None
}

/// Whether Claude Code still has the transcript needed to resume a session.
pub fn transcript_exists(home: &Path, session_id: &str) -> bool {
    transcript_modified(home, session_id).is_some()
}

/// When a session's transcript was last written (its last use) in ms since 1970, or `None` when
/// it has none.
pub fn transcript_modified(home: &Path, session_id: &str) -> Option<f64> {
    let projects = home.join("projects");
    // One after the other: a share does better with that than with hundreds of requests at once.
    for dir in std::fs::read_dir(&projects).ok()?.flatten() {
        let file = dir.path().join(format!("{session_id}.jsonl"));
        let Ok(modified) = std::fs::metadata(&file).and_then(|meta| meta.modified()) else {
            continue;
        };
        let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        return Some(since_epoch.as_secs_f64() * 1000.0);
    }
    None
}
