//! The session list: reframe's own sessions, claudes running outside it, past sessions found in
//! Claude Code's transcripts, and saved workspaces. What the list is made of, how it is searched
//! and sorted, which entries are new, and how it tells when a session was last used.
//!
//! Ported from the Electron version's `src/shared/ipc.ts` (the list's types), the list part of
//! `src/main/app.ts` (`sessionList`), `src/renderer/session-list.ts` (new entries) and
//! `src/renderer/time.ts`. Gathering what goes in (the registry, transcripts, processes) is the
//! app's; this only puts it together.

use std::collections::HashSet;

use crate::i18n::{Locale, t};
use crate::last_used::SessionStatus;
use crate::state::Lifecycle;

/// How many past sessions the list shows at first (and adds with "Show more").
pub const LIST_PAGE: usize = 20;

/// A session reframe manages.
#[derive(Clone, Debug, PartialEq)]
pub struct AppListItem {
    pub id: String,
    pub label: String,
    pub cwd: String,
    pub status: SessionStatus,
    pub lifecycle: Lifecycle,
    /// The window that shows it, and its number (from 1) as the list shows it.
    pub window_id: Option<String>,
    pub window_number: Option<usize>,
    /// When it was last used: a prompt, or Claude done or asking (in another terminal: its
    /// transcript written).
    pub last_active_at: f64,
    /// Not running in reframe, but someone resumed its conversation in another terminal.
    pub elsewhere: bool,
}

/// A claude running outside reframe (from Claude Code's session registry).
#[derive(Clone, Debug, PartialEq)]
pub struct RunningListItem {
    pub pid: u32,
    pub claude_session_id: String,
    pub cwd: String,
    pub name: Option<String>,
    /// When it was last used (its transcript written); without a transcript yet, when it
    /// started.
    pub last_active_at: Option<f64>,
    /// Runs in this WSL distribution (`pid` is a Linux one there).
    pub distro: Option<String>,
}

/// A past session found in Claude Code's transcripts.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundListItem {
    pub claude_session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub modified_at: f64,
    pub distro: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceListItem {
    pub id: String,
    pub name: String,
    pub sessions: usize,
    pub saved_at: f64,
    /// Its window is open.
    pub open: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionList {
    pub workspaces: Vec<WorkspaceListItem>,
    pub app: Vec<AppListItem>,
    pub running: Vec<RunningListItem>,
    pub found: Vec<FoundListItem>,
    /// More past sessions exist beyond the limit asked for.
    pub has_more: bool,
    /// WSL distributions with Claude Code that are not running, so their sessions are missing.
    pub wsl_stopped: Vec<String>,
}

/// What a list entry stands for when it is opened (the details are looked up again then).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ListItemRef {
    App {
        session_id: String,
    },
    Running {
        pid: u32,
        claude_session_id: String,
        distro: Option<String>,
    },
    Found {
        claude_session_id: String,
        distro: Option<String>,
    },
}

impl ListItemRef {
    /// The row's key: one per session reframe manages, one per conversation outside it.
    pub fn key(&self) -> String {
        match self {
            Self::App { session_id } => format!("app:{session_id}"),
            Self::Running { claude_session_id, .. } | Self::Found { claude_session_id, .. } => {
                format!("claude:{claude_session_id}")
            }
        }
    }
}

/// How several list entries are opened at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    /// One after the other.
    Open,
    /// Together in a new group, in this window.
    Group,
    /// Together in a new group, in a new window.
    GroupInNewWindow,
}

impl SessionList {
    /// Every entry's reference, in the order the list shows them.
    pub fn refs(&self) -> Vec<ListItemRef> {
        let app = self.app.iter().map(|item| ListItemRef::App {
            session_id: item.id.clone(),
        });
        let running = self.running.iter().map(|item| ListItemRef::Running {
            pid: item.pid,
            claude_session_id: item.claude_session_id.clone(),
            distro: item.distro.clone(),
        });
        let found = self.found.iter().map(|item| ListItemRef::Found {
            claude_session_id: item.claude_session_id.clone(),
            distro: item.distro.clone(),
        });
        app.chain(running).chain(found).collect()
    }
}

/// Whether any of the fields holds the search, whatever the case. An empty search holds all.
pub fn matches(query: &str, fields: &[Option<&str>]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || fields
            .iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(&query))
}

/// Reframe's own sessions as the list shows them: those the search holds, the ones in a window
/// first, then the rest; the most recently used first within each.
pub fn sort_app(items: Vec<AppListItem>, query: &str) -> Vec<AppListItem> {
    let mut items: Vec<AppListItem> = items
        .into_iter()
        .filter(|item| matches(query, &[Some(&item.label), Some(&item.cwd)]))
        .collect();
    items.sort_by(|a, b| {
        a.window_id
            .is_none()
            .cmp(&b.window_id.is_none())
            .then(b.last_active_at.total_cmp(&a.last_active_at))
    });
    items
}

/// Past sessions from several places (Windows and WSL distributions), newest first, as many as
/// asked for; and whether there are more.
pub fn merge_found(pages: Vec<(Vec<FoundListItem>, bool)>, limit: usize) -> (Vec<FoundListItem>, bool) {
    let mut more = pages.iter().any(|(_, more)| *more);
    let mut found: Vec<FoundListItem> = pages.into_iter().flat_map(|(items, _)| items).collect();
    found.sort_by(|a, b| b.modified_at.total_cmp(&a.modified_at));
    if found.len() > limit {
        more = true;
        found.truncate(limit);
    }
    (found, more)
}

/// Which entries popped up since the list was first made: started in another terminal, or a
/// past one that was used again. Paging and searching only reveal older ones, which are not new.
#[derive(Debug, Default)]
pub struct FreshTracker {
    seen: Option<HashSet<String>>,
    /// When the list was first made (ms since 1970).
    since: f64,
    fresh: HashSet<String>,
}

impl FreshTracker {
    pub fn new(since: f64) -> Self {
        Self {
            seen: None,
            since,
            fresh: HashSet::new(),
        }
    }

    /// Takes a new list; whether new entries popped up.
    pub fn apply(&mut self, list: &SessionList) -> bool {
        let entries = list
            .running
            .iter()
            .map(|item| (item.claude_session_id.as_str(), true))
            .chain(
                list.found
                    .iter()
                    .map(|item| (item.claude_session_id.as_str(), item.modified_at > self.since)),
            );
        let Some(seen) = &mut self.seen else {
            self.seen = Some(entries.map(|(id, _)| id.to_owned()).collect());
            return false;
        };
        let mut popped = false;
        for (id, recent) in entries {
            if seen.insert(id.to_owned()) && recent {
                self.fresh.insert(id.to_owned());
                popped = true;
            }
        }
        popped
    }

    /// Claude session ids of the entries that are new.
    pub fn fresh(&self) -> &HashSet<String> {
        &self.fresh
    }

    /// They were shown long enough: from now on they count as seen.
    pub fn clear(&mut self) {
        self.fresh.clear();
    }
}

/// A moment in the user's time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// The moment as UTC (for tests, and where the time zone is not known).
pub fn utc_time(ms: f64) -> LocalTime {
    let seconds = (ms / 1000.0).floor() as i64;
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    LocalTime {
        year,
        month,
        day,
        hour: (of_day / 3600) as u32,
        minute: (of_day % 3600 / 60) as u32,
    }
}

const MINUTE: f64 = 60_000.0;
const HOUR: f64 = 60.0 * MINUTE;
const DAY: f64 = 24.0 * HOUR;

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_DE: [&str; 12] = [
    "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez.",
];

fn month_name(locale: Locale, month: u32) -> &'static str {
    let index = (month.clamp(1, 12) - 1) as usize;
    match locale {
        Locale::En => MONTHS_EN[index],
        Locale::De => MONTHS_DE[index],
    }
}

/// When a session was last used, short, as its row shows it: "5m", "2h", "3d", then the date
/// (with the year when it was another year). A time a little ahead (another clock) is now.
pub fn short_time(time: f64, now: f64, locale: Locale, local: &dyn Fn(f64) -> LocalTime) -> String {
    let past = (now - time).max(0.0);
    let count = |key: &str, unit: f64| {
        let n = (past / unit).floor().to_string();
        t(locale, key, &[("n", &n)]).unwrap_or(n)
    };
    if past < MINUTE {
        return t(locale, "list.usedNow", &[]).unwrap_or_default();
    }
    if past < HOUR {
        return count("list.usedMinutes", MINUTE);
    }
    if past < DAY {
        return count("list.usedHours", HOUR);
    }
    if past < 7.0 * DAY {
        return count("list.usedDays", DAY);
    }
    let date = local(time);
    let month = month_name(locale, date.month);
    let this_year = date.year == local(now).year;
    match (locale, this_year) {
        (Locale::En, true) => format!("{month} {}", date.day),
        (Locale::De, true) => format!("{}. {month}", date.day),
        (_, false) => format!("{month} {}", date.year),
    }
}

/// How long ago something was, in words, as a saved workspace's row says it: "this minute",
/// "5 minutes ago", "2 hours ago", "yesterday", "3 days ago" (rounded, as
/// `Intl.RelativeTimeFormat` with `numeric: 'auto'` says it). A time a little ahead (another
/// clock) is now.
pub fn time_ago(time: f64, now: f64, locale: Locale) -> String {
    // 0.x rounded the time from now, which is below zero, with Math.round (halves go up):
    // 1.5 days ago was -1, yesterday.
    let round = |past: f64| (past - 0.5).ceil().max(0.0) as u64;
    let text = |key: &str, n: u64| {
        let n = grouped(n, locale);
        t(locale, key, &[("n", &n)]).unwrap_or(n)
    };
    let minutes = round((now - time).max(0.0) / MINUTE);
    if minutes < 60 {
        return match minutes {
            0 => text("list.agoThisMinute", 0),
            1 => text("list.agoMinute", 1),
            n => text("list.agoMinutes", n),
        };
    }
    let hours = round(minutes as f64 / 60.0);
    if hours < 24 {
        return match hours {
            1 => text("list.agoHour", 1),
            n => text("list.agoHours", n),
        };
    }
    match round(hours as f64 / 24.0) {
        1 => text("list.agoYesterday", 1),
        2 => text("list.agoTwoDays", 2),
        n => text("list.agoDays", n),
    }
}

/// A whole number with its thousands marked, as the language writes it: 1,234 or 1.234.
fn grouped(n: u64, locale: Locale) -> String {
    let digits = n.to_string();
    let mark = match locale {
        Locale::En => ',',
        Locale::De => '.',
    };
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(mark);
        }
        out.push(digit);
    }
    out
}

/// When a session was last used, exactly, for hovering its row: "Nov 9, 2026, 3:00 PM" or
/// "09.11.2026, 15:00".
pub fn exact_time(time: f64, locale: Locale, local: &dyn Fn(f64) -> LocalTime) -> String {
    let date = local(time);
    match locale {
        Locale::En => {
            let hour = match date.hour % 12 {
                0 => 12,
                hour => hour,
            };
            let half = if date.hour < 12 { "AM" } else { "PM" };
            format!(
                "{} {}, {}, {hour}:{:02} {half}",
                month_name(locale, date.month),
                date.day,
                date.year,
                date.minute
            )
        }
        Locale::De => format!(
            "{:02}.{:02}.{}, {:02}:{:02}",
            date.day, date.month, date.year, date.hour, date.minute
        ),
    }
}

/// The last part of a folder, as rows name it.
pub fn folder_name(cwd: &str) -> &str {
    cwd.split(['\\', '/']).rfind(|part| !part.is_empty()).unwrap_or(cwd)
}

#[cfg(test)]
#[path = "session_list_tests.rs"]
mod tests;
