//! Finds Claude Code sessions outside reframe: running ones (session registry) and past ones (transcripts
//! in `~/.claude/projects/<project>/<session id>.jsonl`). Transcripts can be megabytes, so only their start
//! and end are read, and results are cached by modification time.
//!
//! Ported from the Electron version's `src/main/claude/discovery.ts`. That file reads without blocking where a
//! WSL distribution's folder may be behind a network share (and has `listRunning` twice for that). Here
//! everything blocks, and the caller runs it off the UI thread. Whether a claude runs is asked through a
//! closure, so this crate stays free of platform code.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Value};

use crate::claude::registry::{RegistryEntry, parse_registry_entry};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunningExclusion {
    pub pids: HashSet<u32>,
    pub session_ids: HashSet<String>,
}

/// `<digits>.json`, as the registry names its files.
fn is_registry_file(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()))
}

/// A registry file's entry, unless it is one to leave out; `None` also for a file being written or removed now.
fn registry_entry(text: &str, exclude: &RunningExclusion) -> Option<RegistryEntry> {
    let entry = parse_registry_entry(&parse_json(text)?)?;
    (!exclude.pids.contains(&entry.pid) && !exclude.session_ids.contains(&entry.session_id)).then_some(entry)
}

/// Claudes that run on this system, or in a WSL distribution when `home` is its folder (`alive`: asked through
/// its /proc there). `alive` is what tells a running claude's entry from one a killed claude left behind
/// (`registry::is_running_entry` with the system's process functions).
pub fn list_running(
    home: &Path,
    exclude: &RunningExclusion,
    alive: &dyn Fn(&RegistryEntry) -> bool,
) -> Vec<RegistryEntry> {
    let sessions = home.join("sessions");
    let Ok(files) = std::fs::read_dir(&sessions) else {
        return Vec::new();
    };
    let mut running = Vec::new();
    // One after the other: a share does better with that than with many requests at once.
    for file in files.flatten() {
        let name = file.file_name();
        let Some(name) = name.to_str().filter(|name| is_registry_file(name)) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(sessions.join(name)) else {
            continue;
        };
        if let Some(entry) = registry_entry(&String::from_utf8_lossy(&bytes), exclude)
            && alive(&entry)
        {
            running.push(entry);
        }
    }
    running
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptInfo {
    pub session_id: String,
    /// When the transcript was last written (ms since 1970).
    pub modified_at: f64,
    pub cwd: Option<String>,
    pub title: Option<String>,
}

/// What a transcript itself tells: a `TranscriptInfo` without the session id and the time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptSummary {
    pub cwd: Option<String>,
    pub title: Option<String>,
}

const HEAD_BYTES: u64 = 64 * 1024;
const TAIL_BYTES: u64 = 256 * 1024;
const TITLE_LENGTH: usize = 90;

type Entry = Map<String, Value>;

/// cwd and a title from a transcript's first and last lines. `Ok(None)` when it holds no prompt; an error when
/// it cannot be read.
pub fn read_transcript_info(file: &Path) -> io::Result<Option<TranscriptSummary>> {
    let mut handle = File::open(file)?;
    let size = handle.metadata()?.len();
    let head = read_chunk(&mut handle, 0, size.min(HEAD_BYTES))?;
    let mut tail = String::new();
    if size > HEAD_BYTES {
        let start = HEAD_BYTES.max(size.saturating_sub(TAIL_BYTES));
        tail = read_chunk(&mut handle, start, size - start)?;
        // drop the partial first line (all of it stays when there is no line break)
        if let Some(line_break) = tail.find('\n') {
            tail.drain(..=line_break);
        }
    }
    drop(handle);
    let mut entries = parse_lines(&head);
    entries.extend(parse_lines(&tail));
    Ok(summary(&entries))
}

fn summary(entries: &[Entry]) -> Option<TranscriptSummary> {
    let first_prompt = first_prompt(entries);
    let last_prompt = last(entries, "last-prompt", "lastPrompt");
    if first_prompt.is_none() && last_prompt.is_none() {
        return None;
    }
    let cwd = entries
        .iter()
        .filter(|entry| is_type(entry, "user"))
        .find_map(|entry| entry.get("cwd").and_then(Value::as_str));
    let title = last(entries, "agent-name", "agentName")
        .or_else(|| last(entries, "ai-title", "aiTitle"))
        .or(last_prompt)
        .or(first_prompt);
    Some(TranscriptSummary {
        cwd: cwd.map(str::to_owned),
        title: title.map(|title| truncate(&collapse_whitespace(title), TITLE_LENGTH)),
    })
}

fn is_type(entry: &Entry, kind: &str) -> bool {
    entry.get("type").and_then(Value::as_str) == Some(kind)
}

/// The latest entry of a type that has text in a field, trimmed.
fn last<'a>(entries: &'a [Entry], kind: &str, field: &str) -> Option<&'a str> {
    entries
        .iter()
        .rev()
        .filter(|entry| is_type(entry, kind))
        .filter_map(|entry| entry.get(field).and_then(Value::as_str))
        .map(js_trim)
        .find(|value| !value.is_empty())
}

/// The first thing the user typed: not a command or other markup, which starts with `<`.
fn first_prompt(entries: &[Entry]) -> Option<&str> {
    entries
        .iter()
        .filter(|entry| is_type(entry, "user"))
        .filter_map(|entry| entry.get("message")?.as_object()?.get("content")?.as_str())
        .find(|content| !content.starts_with('<') && !js_trim(content).is_empty())
}

/// Reads `length` bytes from `position`. What cannot be read (a file that got shorter meanwhile) stays zero, as
/// in the buffer the Electron version reads into; such a line is no JSON and is skipped. Bytes that are no
/// UTF-8 (a character cut in half at a chunk's edge) become U+FFFD.
fn read_chunk(handle: &mut File, position: u64, length: u64) -> io::Result<String> {
    let length = usize::try_from(length).map_err(io::Error::other)?;
    let mut buffer = vec![0_u8; length];
    handle.seek(SeekFrom::Start(position))?;
    let mut filled = 0;
    while filled < length {
        match handle.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn parse_lines(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        if js_trim(line).is_empty() {
            continue;
        }
        // Not JSON: a cut-off line at a chunk boundary. Arrays are objects to JavaScript, but have no type and
        // so never count; they are left out here.
        if let Some(Value::Object(entry)) = parse_json(line) {
            out.push(entry);
        }
    }
    out
}

/// `JSON.parse`, `None` where that throws. JavaScript's strings may hold half a surrogate pair (`"\ud83d"`,
/// which Claude Code writes when it cuts a text in the middle of an emoji) and Rust's cannot: such a half
/// becomes U+FFFD instead of the line being lost.
fn parse_json(text: &str) -> Option<Value> {
    serde_json::from_str(text)
        .ok()
        .or_else(|| match replace_lone_surrogates(text) {
            Cow::Owned(replaced) => serde_json::from_str(&replaced).ok(),
            Cow::Borrowed(_) => None,
        })
}

/// The `\uXXXX` escape that starts at `at`, as its UTF-16 code unit.
fn unicode_escape(bytes: &[u8], at: usize) -> Option<u16> {
    let escape = bytes.get(at..at + 6)?;
    if escape[0] != b'\\' || escape[1] != b'u' || !escape[2..].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    u16::from_str_radix(std::str::from_utf8(&escape[2..]).ok()?, 16).ok()
}

/// JSON text with every `\uD800`–`\uDFFF` escape that is not half of a pair replaced by `�`.
fn replace_lone_surrogates(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
        // In JSON a backslash is only found in a string, where it starts an escape of two or six bytes.
        let Some(unit) = unicode_escape(bytes, at) else {
            at += 2;
            continue;
        };
        let pair = (0xD800..0xDC00).contains(&unit)
            && unicode_escape(bytes, at + 6).is_some_and(|next| (0xDC00..0xE000).contains(&next));
        if pair {
            at += 12;
        } else if (0xD800..0xE000).contains(&unit) {
            // `at` is at a backslash and six ASCII bytes follow: both ends are character boundaries.
            out.push_str(&text[copied..at]);
            out.push_str("\\ufffd");
            at += 6;
            copied = at;
        } else {
            at += 6;
        }
    }
    if copied == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// What JavaScript's `trim()` and `\s` call white space: Unicode's, with U+FEFF and without U+0085.
fn is_js_whitespace(c: char) -> bool {
    c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{85}')
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `replace(/\s+/g, ' ').trim()`: every run of white space is one space, and none at the ends.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split(is_js_whitespace).filter(|word| !word.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// At most `n` long, ending in `…` when it was cut. The length is counted in UTF-16 code units, as JavaScript's
/// `length` and `slice(0, n - 1)` count, but a character is never split: one that the cut would go through (it
/// takes two units) is dropped whole.
fn truncate(s: &str, n: usize) -> String {
    if s.encode_utf16().count() <= n {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut units = 0;
    for c in s.chars() {
        units += c.len_utf16();
        if units > n.saturating_sub(1) {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

#[derive(Clone, Debug)]
struct TranscriptFile {
    file: PathBuf,
    session_id: String,
    modified_at: f64,
}

struct Cached {
    modified_at: f64,
    info: Option<TranscriptInfo>,
}

/// Project folders to list again.
enum Stale {
    All,
    Named(HashSet<String>),
}

pub struct ListOptions<'a> {
    /// Only sessions whose title or folder holds this, whatever the case.
    pub query: Option<&'a str>,
    pub limit: usize,
    /// Session ids to leave out.
    pub exclude: &'a HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptPage {
    pub items: Vec<TranscriptInfo>,
    pub has_more: bool,
}

/// Past sessions, newest first, with search and paging. A project folder's transcripts are listed once and again
/// only after `invalidate()` names that folder (or `invalidate_all()`): looking at every transcript's time each
/// time is what a long history with thousands of them cannot afford.
///
/// The caller owns it and keeps it between scans. (The Electron version answers a scan that is asked for while
/// one runs with that one's result; here a scan borrows the index for as long as it takes, so there is no
/// second one.)
pub struct TranscriptIndex {
    home: PathBuf,
    cache: HashMap<PathBuf, Cached>,
    /// Each project folder's transcripts, as last listed.
    listed: IndexMap<String, Vec<TranscriptFile>>,
    stale: Stale,
}

impl TranscriptIndex {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            cache: HashMap::new(),
            listed: IndexMap::new(),
            stale: Stale::All,
        }
    }

    /// Project folders whose transcripts changed: they are listed again.
    pub fn invalidate<I>(&mut self, projects: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        if let Stale::Named(stale) = &mut self.stale {
            stale.extend(projects.into_iter().map(Into::into));
        }
    }

    /// When it is not known which project folders changed: all are listed again.
    pub fn invalidate_all(&mut self) {
        self.stale = Stale::All;
    }

    pub fn list(&mut self, options: &ListOptions) -> TranscriptPage {
        let files = self.files();
        let query = options.query.map(|query| js_trim(query).to_lowercase());
        let query = query.filter(|query| !query.is_empty());
        let mut items = Vec::new();
        for file in files.iter().filter(|file| !options.exclude.contains(&file.session_id)) {
            let Some(info) = self.info(file) else {
                continue;
            };
            if let Some(query) = &query {
                let holds = |value: &Option<String>| value.as_ref().is_some_and(|v| v.to_lowercase().contains(query));
                if !holds(&info.title) && !holds(&info.cwd) {
                    continue;
                }
            }
            if items.len() == options.limit {
                return TranscriptPage { items, has_more: true };
            }
            items.push(info);
        }
        TranscriptPage { items, has_more: false }
    }

    /// One past session by id, e.g. when it is opened from the list.
    pub fn find(&mut self, session_id: &str) -> Option<TranscriptInfo> {
        let files = self.files();
        let file = files.iter().find(|file| file.session_id == session_id)?;
        self.info(file)
    }

    /// When each session's transcript was last written (without reading them), by session id. In the order of
    /// the transcripts, newest first; of two with one id (in two project folders), the place is the newer one's
    /// and the time the older one's, as in the Electron version's `Map`.
    pub fn modified_times(&mut self) -> IndexMap<String, f64> {
        let mut times = IndexMap::new();
        for file in self.files() {
            times.insert(file.session_id, file.modified_at);
        }
        times
    }

    /// The transcripts, newest first (in WSL they are behind a network share: call this off the UI thread).
    fn files(&mut self) -> Vec<TranscriptFile> {
        let projects = self.home.join("projects");
        let stale = std::mem::replace(&mut self.stale, Stale::Named(HashSet::new()));
        let dirs: IndexSet<String> = match std::fs::read_dir(&projects) {
            Ok(dirs) => dirs
                .flatten()
                .map(|dir| dir.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => IndexSet::new(),
        };
        self.listed.retain(|dir, _| dirs.contains(dir));
        // One folder after the other, and in each one transcript after the other: a share does better with that
        // than with hundreds of requests at once.
        for dir in &dirs {
            if let Stale::Named(stale) = &stale
                && !stale.contains(dir)
                && self.listed.contains_key(dir)
            {
                continue;
            }
            self.listed.insert(dir.clone(), list_project(&projects, dir));
        }
        let mut files: Vec<TranscriptFile> = self.listed.values().flatten().cloned().collect();
        files.sort_by(|a, b| b.modified_at.total_cmp(&a.modified_at));
        // What was read from transcripts that are gone is forgotten.
        let present: HashSet<&Path> = files.iter().map(|file| file.file.as_path()).collect();
        self.cache.retain(|file, _| present.contains(file.as_path()));
        files
    }

    fn info(&mut self, file: &TranscriptFile) -> Option<TranscriptInfo> {
        if let Some(cached) = self.cache.get(&file.file)
            && cached.modified_at == file.modified_at
        {
            return cached.info.clone();
        }
        // An error: unreadable right now; try again next time (which is once its time changed: until then
        // the cache answers).
        let info = read_transcript_info(&file.file)
            .ok()
            .flatten()
            .map(|read| TranscriptInfo {
                session_id: file.session_id.clone(),
                modified_at: file.modified_at,
                cwd: read.cwd,
                title: read.title,
            });
        let cached = Cached {
            modified_at: file.modified_at,
            info: info.clone(),
        };
        self.cache.insert(file.file.clone(), cached);
        info
    }
}

/// The transcripts in one project folder, with their times.
fn list_project(projects: &Path, dir: &str) -> Vec<TranscriptFile> {
    let Ok(names) = std::fs::read_dir(projects.join(dir)) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for name in names.flatten() {
        let name = name.file_name().to_string_lossy().into_owned();
        let Some(session_id) = name.strip_suffix(".jsonl") else {
            continue;
        };
        let file = projects.join(dir).join(&name);
        // An error: removed in the meantime.
        if let Ok(modified) = std::fs::metadata(&file).and_then(|meta| meta.modified()) {
            let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            files.push(TranscriptFile {
                file,
                session_id: session_id.to_owned(),
                modified_at: since_epoch.as_secs_f64() * 1000.0,
            });
        }
    }
    files
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
