//! The session list: what it shows, and keeping it up to date. Windows get it from here when it
//! changed, instead of each asking every few seconds: that read every registry entry and looked
//! at every transcript's time (thousands in a long history) all the time, with or without
//! anything changing. Claude Code's folder is watched instead; what runs outside reframe is
//! looked at again every little while too (a killed claude changes no file).
//!
//! Ported from the Electron version's `src/main/app.ts` (`sessionList`, `watchList`,
//! `scanOutside`, `listChanged`). WSL distributions follow with the WSL milestone.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::oneshot;
use gpui_kit::*;
use indexmap::IndexMap;
use reframe_core::actions::window_sessions;
use reframe_core::claude::registry::{RegistryEntry, is_running_entry};
use reframe_core::discovery::{ListOptions, RunningExclusion, TranscriptPage, list_running};
use reframe_core::session_list::{
    AppListItem, FoundListItem, LIST_PAGE, LocalTime, RunningListItem, SessionList, WorkspaceListItem, matches,
    merge_found, sort_app,
};
use reframe_core::state::{WindowState, Workspace};

use super::{AppEvent, Controller};
use crate::process::{is_alive, process_start};
use crate::watcher::{self, Changes, Timing};

/// How often what runs outside reframe is looked at, besides when Claude Code's folder changes.
const LIST_LOOK: Duration = Duration::from_millis(20_000);
/// How long after a change the windows get the list (many changes come together).
const LIST_PUSH: Duration = Duration::from_millis(300);

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
                if this.update(cx, |this, cx| drop(this.scan_outside(cx))).is_err() {
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
        drop(self.scan_outside(cx));
    }

    /// Looks again what runs outside reframe; the windows then get the list if it changed. One
    /// look at a time: asked for meanwhile, another one follows. The answer comes once this one
    /// is done.
    pub(super) fn scan_outside(&mut self, cx: &mut Context<Self>) -> oneshot::Receiver<()> {
        let (done, receiver) = oneshot::channel();
        self.scan_waiters.push(done);
        if self.scanning {
            self.rescan = true;
            return receiver;
        }
        self.scanning = true;
        let home = self.claude_home.clone();
        let exclusion = RunningExclusion {
            pids: self.sessions.read(cx).own_pids(),
            session_ids: HashSet::new(),
        };
        cx.spawn(async move |this, cx| {
            let running = cx
                .background_executor()
                .spawn(async move {
                    let alive = |entry: &RegistryEntry| is_running_entry(entry, &is_alive, &process_start);
                    list_running(&home, &exclusion, &alive)
                        .into_iter()
                        .filter(|entry| entry.cwd.as_deref().is_some_and(|cwd| !cwd.is_empty()))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.outside = Some(running);
                this.scanning = false;
                for waiter in std::mem::take(&mut this.scan_waiters) {
                    let _ = waiter.send(());
                }
                if std::mem::take(&mut this.rescan) {
                    drop(this.scan_outside(cx));
                }
                this.list_changed(cx);
            });
        })
        .detach();
        receiver
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
        let scanned = self.outside.is_none().then(|| self.scan_outside(cx));
        cx.spawn(async move |this, cx| {
            if let Some(scanned) = scanned {
                let _ = scanned.await;
            }
            let Ok((transcripts, running, mut exclude)) = this.update(cx, |this, _| {
                let own: HashSet<String> = this
                    .state
                    .sessions
                    .values()
                    .map(|record| record.claude_session_id.clone())
                    .collect();
                (this.transcripts.clone(), this.outside.clone().unwrap_or_default(), own)
            }) else {
                return SessionList::default();
            };
            exclude.extend(running.iter().map(|entry| entry.session_id.clone()));
            let asked = query.clone();
            let any_running = !running.is_empty();
            let read = cx.background_executor().spawn(async move {
                let Ok(mut index) = transcripts.lock() else {
                    return (
                        IndexMap::new(),
                        TranscriptPage {
                            items: Vec::new(),
                            has_more: false,
                        },
                    );
                };
                // When what runs outside reframe was last used: when its transcript was last
                // written.
                let times = if any_running {
                    index.modified_times()
                } else {
                    IndexMap::new()
                };
                let options = ListOptions {
                    query: Some(&asked),
                    limit,
                    exclude: &exclude,
                };
                let page = index.list(&options);
                (times, page)
            });
            let (times, page) = read.await;
            this.update(cx, |this, cx| {
                this.build_list(&query, limit, &running, &times, page, cx)
            })
            .unwrap_or_default()
        })
    }

    fn build_list(
        &self,
        query: &str,
        limit: usize,
        running: &[RegistryEntry],
        times: &IndexMap<String, f64>,
        page: TranscriptPage,
        cx: &App,
    ) -> SessionList {
        let sessions = self.sessions.read(cx);
        let own: HashSet<&str> = self
            .state
            .sessions
            .values()
            .map(|record| record.claude_session_id.as_str())
            .collect();
        // Reframe's own sessions that someone resumed elsewhere are among what runs there.
        let resumed_elsewhere: HashSet<&str> = running.iter().map(|entry| entry.session_id.as_str()).collect();
        let used_outside = |entry: &RegistryEntry| times.get(&entry.session_id).copied().or(entry.started_at);
        let used_outside_of = |claude_id: &str| {
            running
                .iter()
                .find(|entry| entry.session_id == claude_id)
                .and_then(used_outside)
        };
        let app: Vec<AppListItem> = self
            .state
            .sessions
            .values()
            .filter_map(|record| {
                let view = self.session_view(&record.id, cx)?;
                let window = self.window_of(&record.id);
                let elsewhere = resumed_elsewhere.contains(record.claude_session_id.as_str())
                    && !sessions.keeps_running(&record.id);
                // Resumed in another terminal, it may have been used there since.
                let there = if elsewhere {
                    used_outside_of(&record.claude_session_id).unwrap_or(0.0)
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
        let running_items: Vec<RunningListItem> = running
            .iter()
            .filter(|entry| !own.contains(entry.session_id.as_str()))
            .map(|entry| RunningListItem {
                pid: entry.pid,
                claude_session_id: entry.session_id.clone(),
                cwd: entry.cwd.clone().unwrap_or_default(),
                name: entry.name.clone(),
                last_active_at: used_outside(entry),
                distro: None,
            })
            .filter(|item| matches(query, &[item.name.as_deref(), Some(&item.cwd)]))
            .collect();
        let found: Vec<FoundListItem> = page
            .items
            .into_iter()
            .filter_map(|info| {
                Some(FoundListItem {
                    cwd: info.cwd.filter(|cwd| !cwd.is_empty())?,
                    claude_session_id: info.session_id,
                    title: info.title,
                    modified_at: info.modified_at,
                    distro: None,
                })
            })
            .collect();
        let (found, has_more) = merge_found(vec![(found, page.has_more)], limit);
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
            wsl_stopped: Vec::new(),
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
