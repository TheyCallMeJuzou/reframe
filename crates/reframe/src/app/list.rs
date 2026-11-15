//! The session list: what it shows, and keeping it up to date. Windows get it from here when it
//! changed, instead of each asking every few seconds: that read every registry entry and looked
//! at every transcript's time (thousands in a long history) and asked wsl.exe which distributions
//! run, all the time, with or without anything changing. Windows' own Claude Code folder is
//! watched instead; what runs outside reframe (and in WSL) is looked at again every little while.
//!
//! Ported from the Electron version's `src/main/app.ts` (`sessionList`, `claudeSources`,
//! `watchList`, `scanOutside`, `listChanged`).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::oneshot;
use gpui_kit::*;
use indexmap::IndexMap;
use reframe_core::actions::window_sessions;
use reframe_core::claude::registry::{RegistryEntry, is_running_entry};
use reframe_core::discovery::{ListOptions, RunningExclusion, TranscriptIndex, TranscriptPage, list_running};
use reframe_core::session_list::{
    AppListItem, FoundListItem, LIST_PAGE, LocalTime, RunningListItem, SessionList, WorkspaceListItem, matches,
    merge_found, sort_app,
};
use reframe_core::state::{WindowState, Workspace};
use reframe_core::wsl::wsl_windows_path;

use super::{AppEvent, Controller};
use crate::process::{is_alive, process_start};
use crate::watcher::{self, Changes, Timing};
use crate::wsl::WslDistros;

/// How often what runs outside reframe is looked at, besides when Claude Code's folder changes.
const LIST_LOOK: Duration = Duration::from_millis(20_000);
/// How often WSL is looked at: while a session list is shown, and while none is.
const WSL_LOOK_SHOWN: Duration = Duration::from_millis(20_000);
const WSL_LOOK_HIDDEN: Duration = Duration::from_millis(60_000);
/// How long after a change the windows get the list (many changes come together).
const LIST_PUSH: Duration = Duration::from_millis(300);

/// Where Claude Code keeps sessions and transcripts: on Windows, or in a running WSL
/// distribution; and what runs there outside reframe.
#[derive(Clone)]
pub(super) struct Source {
    pub distro: Option<String>,
    /// Claude Code's folder, as Windows reaches it.
    pub home: PathBuf,
    pub transcripts: Arc<Mutex<TranscriptIndex>>,
    /// Claudes running outside reframe (with a known folder, as a Linux path in WSL).
    pub running: Vec<RegistryEntry>,
}

/// What was found outside reframe the last time it was looked at.
#[derive(Clone, Default)]
pub(super) struct OutsideScan {
    /// Windows first.
    pub sources: Vec<Source>,
    /// Distributions Claude Code was found in before that do not run now.
    pub stopped: Vec<String>,
}

/// A folder of a source as Windows reaches it.
fn windows_path(distro: Option<&str>, folder: &str, share: &str) -> String {
    match distro {
        Some(distro) => wsl_windows_path(distro, folder, share),
        None => folder.to_owned(),
    }
}

/// Claudes running in a source outside reframe (`own`: reframe's processes there), with a folder.
fn running_in(home: &std::path::Path, own: HashSet<u32>, alive: &dyn Fn(&RegistryEntry) -> bool) -> Vec<RegistryEntry> {
    let exclusion = RunningExclusion {
        pids: own,
        session_ids: HashSet::new(),
    };
    list_running(home, &exclusion, alive)
        .into_iter()
        .filter(|entry| entry.cwd.as_deref().is_some_and(|cwd| !cwd.is_empty()))
        .collect()
}

/// The running distributions with Claude Code, as sources (their transcript indexes kept from
/// `indexes`), and the known ones that do not run.
fn scan_wsl(
    wsl: &WslDistros,
    max_age: Duration,
    known: &[String],
    own_linux: &HashSet<u32>,
    indexes: &HashMap<PathBuf, Arc<Mutex<TranscriptIndex>>>,
) -> (Vec<Source>, Vec<String>) {
    let running = wsl.running(Some(max_age));
    let mut sources = Vec::new();
    for distro in &running {
        let Some(info) = wsl.info(distro).filter(|info| info.claude.is_some()) else {
            continue;
        };
        let home = PathBuf::from(wsl_windows_path(distro, &info.claude_home, &wsl.share));
        let transcripts = indexes
            .get(&home)
            .cloned()
            .unwrap_or_else(|| Arc::new(Mutex::new(TranscriptIndex::new(home.clone()))));
        // Not watched: listed again with every look.
        if let Ok(mut index) = transcripts.lock() {
            index.invalidate_all();
        }
        let alive = |entry: &RegistryEntry| wsl.is_claude(distro, entry.pid);
        let running = running_in(&home, own_linux.clone(), &alive);
        sources.push(Source {
            distro: Some(distro.clone()),
            home,
            transcripts,
            running,
        });
    }
    let stopped = known
        .iter()
        .filter(|name| !running.iter().any(|running| running.eq_ignore_ascii_case(name)))
        .cloned()
        .collect();
    (sources, stopped)
}

impl Controller {
    /// Watches Claude Code's folder and looks at what runs outside reframe every little while.
    pub(super) fn watch_list(&mut self, cx: &mut Context<Self>) {
        let mut changes = watcher::watch(self.claude_home.clone(), Timing::default());
        let watching = cx.spawn(async move |this, cx| {
            while let Some(changes) = changes.next().await {
                if this.update(cx, |this, cx| this.claude_changed(changes, cx)).is_err() {
                    break;
                }
            }
        });
        let looking = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LIST_LOOK).await;
                let looked = this.update(cx, |this, cx| {
                    let shown = this
                        .state
                        .windows
                        .iter()
                        .any(|window| window.sidebar_open == Some(true) && !window.hidden);
                    let age = if shown { WSL_LOOK_SHOWN } else { WSL_LOOK_HIDDEN };
                    drop(this.scan_outside(Some(age), cx));
                });
                if looked.is_err() {
                    break;
                }
            }
        });
        self.list_tasks = vec![watching, looking];
    }

    fn claude_changed(&mut self, changes: Changes, cx: &mut Context<Self>) {
        let transcripts = self.transcripts.clone();
        // The index may be busy listing (it waits for that), so this is not done here.
        cx.background_executor()
            .spawn(async move {
                let Ok(mut index) = transcripts.lock() else { return };
                match changes.projects {
                    Some(projects) => index.invalidate(projects),
                    None => index.invalidate_all(),
                }
            })
            .detach();
        drop(self.scan_outside(None, cx));
    }

    /// Looks again what runs outside reframe: on Windows always (cheap), in WSL when that was
    /// last done `wsl_max_age` ago or longer (`None`: not now; the first look always). The
    /// windows then get the list if it changed. One look at a time: asked for meanwhile, another
    /// one follows. The answer comes once this one is done.
    pub(super) fn scan_outside(
        &mut self,
        wsl_max_age: Option<Duration>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<()> {
        let (done, receiver) = oneshot::channel();
        self.scan_waiters.push(done);
        if self.scanning {
            self.rescan = true;
            if let Some(age) = wsl_max_age {
                self.rescan_wsl = Some(self.rescan_wsl.map_or(age, |known| known.min(age)));
            }
            return receiver;
        }
        self.scanning = true;
        let first = self.outside.is_none();
        let wsl_age = if first { Some(Duration::ZERO) } else { wsl_max_age };
        let wsl_due = wsl_age.filter(|age| self.wsl_looked_at.is_none_or(|at| at.elapsed() >= *age));
        let home = self.claude_home.clone();
        let own = self.sessions.read(cx).own_pids();
        // In WSL reframe's own claudes are known by their Linux ids.
        let own_linux: HashSet<u32> = {
            let sessions = self.sessions.read(cx);
            self.state
                .sessions
                .values()
                .filter(|record| sessions.status(&record.id).is_some())
                .filter_map(|record| record.linux_pid)
                .collect()
        };
        let known = self.state.wsl_distros.clone().unwrap_or_default();
        let previous = self.outside.clone().unwrap_or_default();
        let indexes = self.wsl_transcripts.clone();
        let wsl = self.wsl.clone();
        let transcripts = self.transcripts.clone();
        if wsl_due.is_some() {
            self.wsl_looked_at = Some(Instant::now());
        }
        cx.spawn(async move |this, cx| {
            let scan = cx
                .background_executor()
                .spawn(async move {
                    let alive = |entry: &RegistryEntry| is_running_entry(entry, &is_alive, &process_start);
                    let windows = Source {
                        distro: None,
                        running: running_in(&home, own, &alive),
                        home,
                        transcripts,
                    };
                    let (in_wsl, stopped) = match wsl_due {
                        Some(age) => scan_wsl(&wsl, age, &known, &own_linux, &indexes),
                        None => (previous.sources.into_iter().skip(1).collect(), previous.stopped),
                    };
                    OutsideScan {
                        sources: std::iter::once(windows).chain(in_wsl).collect(),
                        stopped,
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                for source in scan.sources.iter().filter(|source| source.distro.is_some()) {
                    this.wsl_transcripts
                        .insert(source.home.clone(), source.transcripts.clone());
                }
                this.outside = Some(scan);
                this.scanning = false;
                this.remember_wsl_distros(cx);
                for waiter in std::mem::take(&mut this.scan_waiters) {
                    let _ = waiter.send(());
                }
                if std::mem::take(&mut this.rescan) {
                    let age = this.rescan_wsl.take();
                    drop(this.scan_outside(age, cx));
                }
                this.list_changed(cx);
            });
        })
        .detach();
        receiver
    }

    /// Starts a WSL distribution from the session list (and keeps it running), so that its
    /// sessions show up.
    pub fn start_wsl(&mut self, distro: &str, cx: &mut Context<Self>) {
        let (wsl, distro) = (self.wsl.clone(), distro.to_owned());
        cx.spawn(async move |this, cx| {
            cx.background_executor().spawn(async move { wsl.keep(&distro) }).await;
            cx.background_executor().timer(Duration::from_millis(2000)).await;
            let _ = this.update(cx, |this, cx| drop(this.scan_outside(Some(Duration::ZERO), cx)));
        })
        .detach();
    }

    /// Where a list entry's Claude Code keeps its sessions: on Windows, or in its WSL
    /// distribution (if that runs).
    pub(super) fn source_of(&self, distro: Option<&str>) -> Option<Source> {
        let outside = self.outside.as_ref();
        match distro {
            None => Some(Source {
                distro: None,
                home: self.claude_home.clone(),
                transcripts: self.transcripts.clone(),
                running: outside
                    .and_then(|scan| scan.sources.first())
                    .map(|source| source.running.clone())
                    .unwrap_or_default(),
            }),
            Some(distro) => outside?
                .sources
                .iter()
                .find(|source| {
                    source
                        .distro
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(distro))
                })
                .cloned(),
        }
    }

    /// Something the session list shows may have changed: the windows get it in a moment, if it
    /// did.
    pub(super) fn list_changed(&mut self, cx: &mut Context<Self>) {
        if self.list_push || self.shutting_down {
            return;
        }
        self.list_push = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIST_PUSH).await;
            let Ok(list) = this.update(cx, |this, cx| {
                this.list_push = false;
                this.session_list(String::new(), LIST_PAGE, cx)
            }) else {
                return;
            };
            let list = list.await;
            let _ = this.update(cx, |this, cx| {
                if this.shutting_down || this.list_sent.as_deref() == Some(&list) {
                    return;
                }
                this.list_sent = Some(Rc::new(list));
                cx.emit(AppEvent::SessionList);
            });
        })
        .detach();
    }

    /// The list's first page as the windows got it last; `None` before the first one is made.
    pub fn first_page(&self) -> Option<Rc<SessionList>> {
        self.list_sent.clone()
    }

    /// The session list: those of reframe's sessions the search holds, what runs outside reframe,
    /// and as many past sessions as asked for.
    pub fn session_list(&mut self, query: String, limit: usize, cx: &mut Context<Self>) -> Task<SessionList> {
        let scanned = self.outside.is_none().then(|| self.scan_outside(None, cx));
        cx.spawn(async move |this, cx| {
            if let Some(scanned) = scanned {
                let _ = scanned.await;
            }
            let Ok((scan, mut exclude)) = this.update(cx, |this, _| {
                let own: HashSet<String> = this
                    .state
                    .sessions
                    .values()
                    .map(|record| record.claude_session_id.clone())
                    .collect();
                (this.outside.clone().unwrap_or_default(), own)
            }) else {
                return SessionList::default();
            };
            for source in &scan.sources {
                exclude.extend(source.running.iter().map(|entry| entry.session_id.clone()));
            }
            let asked = query.clone();
            let sources = scan.sources.clone();
            let read = cx.background_executor().spawn(async move {
                sources
                    .iter()
                    .map(|source| {
                        let empty = TranscriptPage {
                            items: Vec::new(),
                            has_more: false,
                        };
                        let Ok(mut index) = source.transcripts.lock() else {
                            return (IndexMap::new(), empty);
                        };
                        // When what runs outside reframe was last used: when its transcript was
                        // last written. Sources where nothing runs are not asked.
                        let times = if source.running.is_empty() {
                            IndexMap::new()
                        } else {
                            index.modified_times()
                        };
                        let options = ListOptions {
                            query: Some(&asked),
                            limit,
                            exclude: &exclude,
                        };
                        (times, index.list(&options))
                    })
                    .collect::<Vec<_>>()
            });
            let read = read.await;
            this.update(cx, |this, cx| this.build_list(&query, limit, &scan, read, cx))
                .unwrap_or_default()
        })
    }

    fn build_list(
        &self,
        query: &str,
        limit: usize,
        scan: &OutsideScan,
        read: Vec<(IndexMap<String, f64>, TranscriptPage)>,
        cx: &App,
    ) -> SessionList {
        let sessions = self.sessions.read(cx);
        let share = self.wsl.share.clone();
        let own: HashSet<&str> = self
            .state
            .sessions
            .values()
            .map(|record| record.claude_session_id.as_str())
            .collect();
        // When each claude outside reframe was last used, and where it runs. Reframe's own
        // sessions that someone resumed elsewhere are among them.
        let mut outside: HashMap<&str, (Option<f64>, &Source, &RegistryEntry)> = HashMap::new();
        for (source, (times, _)) in scan.sources.iter().zip(&read) {
            for entry in &source.running {
                let used = times.get(&entry.session_id).copied().or(entry.started_at);
                outside.insert(entry.session_id.as_str(), (used, source, entry));
            }
        }
        let app: Vec<AppListItem> = self
            .state
            .sessions
            .values()
            .filter_map(|record| {
                let view = self.session_view(&record.id, cx)?;
                let window = self.window_of(&record.id);
                let elsewhere =
                    outside.contains_key(record.claude_session_id.as_str()) && !sessions.keeps_running(&record.id);
                // Resumed in another terminal, it may have been used there since.
                let there = if elsewhere {
                    outside
                        .get(record.claude_session_id.as_str())
                        .and_then(|(used, ..)| *used)
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                Some(AppListItem {
                    id: record.id.clone(),
                    label: view.label,
                    cwd: record.cwd.clone(),
                    status: view.status,
                    lifecycle: record.lifecycle,
                    window_id: window.map(|window| window.id.clone()),
                    window_number: window
                        .and_then(|window| self.state.windows.iter().position(|w| w.id == window.id))
                        .map(|index| index + 1),
                    last_active_at: record.last_active_at.max(there),
                    elsewhere,
                })
            })
            .collect();
        let running_items: Vec<RunningListItem> = scan
            .sources
            .iter()
            .flat_map(|source| source.running.iter().map(move |entry| (source, entry)))
            .filter(|(_, entry)| !own.contains(entry.session_id.as_str()))
            .map(|(source, entry)| RunningListItem {
                pid: entry.pid,
                claude_session_id: entry.session_id.clone(),
                cwd: windows_path(
                    source.distro.as_deref(),
                    entry.cwd.as_deref().unwrap_or_default(),
                    &share,
                ),
                name: entry.name.clone(),
                last_active_at: outside.get(entry.session_id.as_str()).and_then(|(used, ..)| *used),
                distro: source.distro.clone(),
            })
            .filter(|item| matches(query, &[item.name.as_deref(), Some(&item.cwd)]))
            .collect();
        let pages: Vec<(Vec<FoundListItem>, bool)> = scan
            .sources
            .iter()
            .zip(read)
            .map(|(source, (_, page))| {
                let items = page
                    .items
                    .into_iter()
                    .filter_map(|info| {
                        let cwd = info.cwd.filter(|cwd| !cwd.is_empty())?;
                        Some(FoundListItem {
                            cwd: windows_path(source.distro.as_deref(), &cwd, &share),
                            claude_session_id: info.session_id,
                            title: info.title,
                            modified_at: info.modified_at,
                            distro: source.distro.clone(),
                        })
                    })
                    .collect();
                (items, page.has_more)
            })
            .collect();
        let (found, has_more) = merge_found(pages, limit);
        let workspaces = self
            .state
            .workspaces
            .iter()
            .map(|workspace| WorkspaceListItem {
                id: workspace.id.clone(),
                name: workspace.name.clone(),
                sessions: workspace.sessions.len(),
                saved_at: workspace.saved_at,
                open: self.window_of_workspace(&workspace.id).is_some(),
            })
            .collect();
        SessionList {
            workspaces,
            app: sort_app(app, query),
            running: running_items,
            found,
            has_more,
            wsl_stopped: scan.stopped.clone(),
        }
    }

    /// The workspace a window stands for: the one it was saved as or opened from, as long as it
    /// still shows some of that workspace's sessions (a window that has become something else is
    /// just a window).
    pub fn workspace_of(&self, window: &WindowState) -> Option<&Workspace> {
        let workspace = self
            .state
            .workspaces
            .iter()
            .find(|workspace| Some(&workspace.id) == window.workspace_id.as_ref())?;
        let ids: HashSet<&str> = workspace
            .sessions
            .iter()
            .flat_map(|saved| [saved.id.as_str(), saved.claude_session_id.as_str()])
            .collect();
        let shows = window_sessions(window).iter().any(|id| {
            ids.contains(id.as_str())
                || self
                    .state
                    .sessions
                    .get(id)
                    .is_some_and(|record| ids.contains(record.claude_session_id.as_str()))
        });
        shows.then_some(workspace)
    }

    pub(super) fn window_of_workspace(&self, workspace_id: &str) -> Option<&WindowState> {
        self.state
            .windows
            .iter()
            .find(|window| window.workspace_id.as_deref() == Some(workspace_id) && self.workspace_of(window).is_some())
    }
}

/// A moment in the user's time zone, as Windows knows it.
pub fn local_time(ms: f64) -> LocalTime {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // 100 ns steps since 1601.
    let ticks = ((ms.max(0.0) as u64) + 11_644_473_600_000) * 10_000;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let zero = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    let (mut utc, mut local) = (zero, zero);
    // SAFETY: plain values in and out.
    let told = unsafe {
        FileTimeToSystemTime(&file, &mut utc) != 0
            && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
    };
    if !told {
        return reframe_core::session_list::utc_time(ms);
    }
    LocalTime {
        year: i32::from(local.wYear),
        month: u32::from(local.wMonth),
        day: u32::from(local.wDay),
        hour: u32::from(local.wHour),
        minute: u32::from(local.wMinute),
    }
}
