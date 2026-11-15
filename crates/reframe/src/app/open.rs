//! Opening what the session list shows: reframe's own sessions, and conversations found outside
//! it, which become sessions of reframe's (adopted). One that runs in another terminal is ended
//! there first, after asking, so a conversation never runs twice. Also: starting a session whose
//! conversation someone resumed elsewhere, and workspaces (windows saved under a name).
//!
//! Ported from the Electron version's `src/main/app.ts` (`recordFor`, `adopt`, `openSession`,
//! `openSessions`, `startOrTakeOver`, `saveWorkspace` and the rest of the workspace part).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use futures::FutureExt;
use gpui_kit::*;
use reframe_core::actions::{LayoutAction, apply_layout_action, put_node, take_node, window_groups};
use reframe_core::claude::args::LaunchMode;
use reframe_core::claude::registry::{
    RegistryEntry, find_registry_entry, is_running_entry, read_registry_entry, transcript_modified,
};
use reframe_core::i18n::t;
use reframe_core::layout::{DropTarget, append_node, leaves, tile, tile_key};
use reframe_core::session_list::{ListItemRef, OpenMode, folder_name};
use reframe_core::session_logic::SessionError;
use reframe_core::state::{DockEntry, GroupNode, LaunchOptions, LayoutNode, Lifecycle, SessionRecord};
use reframe_core::workspaces::{snapshot_window, window_from_workspace};
use serde_json::json;
use uuid::Uuid;

use super::{AppEvent, Controller, now_ms};
use crate::console::{StopResult, stop_external_claude};
use crate::process::{is_alive, process_start};
use crate::window::{self, Question};

impl Controller {
    fn ask_takeover(&self, window_id: &str, labels: &[String], cx: &mut Context<Self>) -> Task<bool> {
        let locale = self.locale();
        let text = |key: &str, params: &[(&str, &str)]| t(locale, key, params).unwrap_or_default();
        let message = match labels {
            [label] => text("takeover.message", &[("label", label)]),
            _ => text(
                "takeover.messageMany",
                &[("count", &labels.len().to_string()), ("labels", &labels.join(", "))],
            ),
        };
        let question = Question {
            title: text("takeover.title", &[]),
            message,
            detail: Some(text("takeover.detail", &[])),
            confirm: text("takeover.confirm", &[]),
            cancel: Some(text("dialog.cancel", &[])),
            danger: false,
        };
        let answer = window::ask(window_id, question, cx);
        cx.spawn(async move |_, _| answer.await.unwrap_or(false))
    }

    /// Tells the user that something did not work.
    fn show_error(&self, window_id: &str, message: String, cx: &mut Context<Self>) {
        let question = Question {
            title: "reframe".to_owned(),
            message,
            detail: None,
            confirm: t(self.locale(), "about.close", &[]).unwrap_or_default(),
            cancel: None,
            danger: false,
        };
        drop(window::ask(window_id, question, cx));
    }

    /// Ends a claude that runs outside reframe (off the UI thread); whether it is gone.
    fn stop_outside(&self, pid: u32, cx: &mut Context<Self>) -> Task<bool> {
        let log = self.log.clone();
        cx.background_executor().spawn(async move {
            let warn = |what: &str, detail: String| log.warn(what, Some(json!({ "pid": pid, "detail": detail })));
            stop_external_claude(pid, &warn) != StopResult::Failed
        })
    }

    /// The conversation of a session, running outside reframe: someone resumed it in a terminal
    /// of their own.
    fn running_elsewhere(&self, record: &SessionRecord, cx: &mut Context<Self>) -> Task<Option<RegistryEntry>> {
        let home = self.claude_home.clone();
        let own = self.sessions.read(cx).own_pids();
        let claude_id = record.claude_session_id.clone();
        cx.background_executor().spawn(async move {
            let alive =
                |entry: &RegistryEntry| !own.contains(&entry.pid) && is_running_entry(entry, &is_alive, &process_start);
            find_registry_entry(&home, &claude_id, Some(&alive))
        })
    }

    /// Starts a session (resuming it), unless its conversation runs outside reframe already:
    /// someone resumed it in a terminal of their own. Then it is taken over (ended there,
    /// continued here) after asking in `window_id`'s window. Without a window to ask in (reframe
    /// starting), or when the answer is no, it is not started, and its tile says why and offers
    /// to take it over.
    pub(super) fn start_or_take_over(
        &mut self,
        session_id: &str,
        mode: LaunchMode,
        window_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.starts_pending.contains(session_id) || self.sessions.read(cx).keeps_running(session_id) {
            return;
        }
        let Some(record) = self.state.sessions.get(session_id).cloned() else {
            return;
        };
        self.starts_pending.insert(session_id.to_owned());
        let outside = (mode == LaunchMode::Resume).then(|| self.running_elsewhere(&record, cx));
        let id = session_id.to_owned();
        cx.spawn(async move |this, cx| {
            let outside = match outside {
                Some(outside) => outside.await,
                None => None,
            };
            if let Some(entry) = outside {
                let asked = match &window_id {
                    Some(window_id) => {
                        let label = this.update(cx, |this, cx| {
                            this.session_view(&id, cx).map(|view| view.label).unwrap_or_default()
                        });
                        let question = this.update(cx, |this, cx| {
                            this.ask_takeover(window_id, &[label.unwrap_or_default()], cx)
                        });
                        match question {
                            Ok(question) => question.await,
                            Err(_) => false,
                        }
                    }
                    None => false,
                };
                let _ = this.update(cx, |this, _| {
                    this.log.info(
                        "session runs elsewhere already",
                        Some(json!({ "session": id, "pid": entry.pid, "asked": asked })),
                    );
                });
                let stopped = if asked {
                    match this.update(cx, |this, cx| this.stop_outside(entry.pid, cx)) {
                        Ok(stopping) => stopping.await,
                        Err(_) => false,
                    }
                } else {
                    false
                };
                if !stopped {
                    let _ = this.update(cx, |this, cx| {
                        this.starts_pending.remove(&id);
                        if asked && let Some(window_id) = &window_id {
                            let message = t(this.locale(), "takeover.failed", &[]).unwrap_or_default();
                            this.show_error(window_id, message, cx);
                        }
                        this.sessions.update(cx, |sessions, cx| {
                            sessions.refuse(&id, SessionError::RunningElsewhere, cx)
                        });
                    });
                    return;
                }
                // Resumed in another folder, the conversation went on there; so it does here. It
                // was used there, too.
                let home = this.update(cx, |this, _| this.claude_home.clone()).ok();
                let claude_id = record.claude_session_id.clone();
                let used_there = match home {
                    Some(home) => {
                        cx.background_executor()
                            .spawn(async move { transcript_modified(&home, &claude_id) })
                            .await
                    }
                    None => None,
                };
                let _ = this.update(cx, |this, cx| {
                    this.update_session(&id, true, cx, |latest| {
                        if let Some(cwd) = entry.cwd.clone().filter(|cwd| !cwd.is_empty()) {
                            latest.cwd = cwd;
                        }
                        if let Some(used) = used_there.filter(|used| *used > latest.last_active_at) {
                            latest.last_active_at = used;
                        }
                    });
                });
            }
            let _ = this.update(cx, |this, cx| {
                this.starts_pending.remove(&id);
                // Removed, or its tile closed, while that was asked; or started meanwhile.
                let Some(current) = this.state.sessions.get(&id).cloned() else {
                    return;
                };
                if this.window_of(&id).is_none() || this.sessions.read(cx).keeps_running(&id) {
                    return;
                }
                this.sessions
                    .update(cx, |sessions, cx| sessions.start(&current, mode, cx));
            });
        })
        .detach();
    }

    // ---------- list entries ----------

    /// The record for a list entry: reframe's own, or a new one for a session found or running
    /// elsewhere (not in the state yet). One running elsewhere is ended there first (after
    /// asking, unless already `confirmed`). `None` when there is nothing (any more) to open.
    fn record_for(
        &mut self,
        window_id: &str,
        item: &ListItemRef,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) -> Task<Option<SessionRecord>> {
        let claude_id = match item {
            ListItemRef::App { session_id } => return Task::ready(self.state.sessions.get(session_id).cloned()),
            ListItemRef::Running { claude_session_id, .. } | ListItemRef::Found { claude_session_id, .. } => {
                claude_session_id.clone()
            }
        };
        // A conversation reframe already manages is opened as that session, not a second copy.
        if let Some(known) = self
            .state
            .sessions
            .values()
            .find(|record| record.claude_session_id == claude_id)
        {
            return Task::ready(Some(known.clone()));
        }
        // Asked for twice at once (two clicks, a click and a drop): one adoption, which both get.
        let adopting = match self.adopting.get(&claude_id) {
            Some(adopting) => adopting.clone(),
            None => {
                let adopting = self.adopt(window_id, item, confirmed, cx).shared();
                self.adopting.insert(claude_id.clone(), adopting.clone());
                adopting
            }
        };
        cx.spawn(async move |this, cx| {
            let record = adopting.await;
            let _ = this.update(cx, |this, _| this.adopting.remove(&claude_id));
            record
        })
    }

    /// A new record for a session found or running elsewhere (see `record_for`).
    fn adopt(
        &mut self,
        window_id: &str,
        item: &ListItemRef,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) -> Task<Option<SessionRecord>> {
        let home = self.claude_home.clone();
        let transcripts = self.transcripts.clone();
        let window_id = window_id.to_owned();
        match item.clone() {
            ListItemRef::App { .. } => Task::ready(None),
            ListItemRef::Found { claude_session_id, .. } => cx.spawn(async move |this, cx| {
                let id = claude_session_id.clone();
                let info = cx
                    .background_executor()
                    .spawn(async move { transcripts.lock().ok().and_then(|mut index| index.find(&id)) })
                    .await;
                match info.and_then(|info| info.cwd).filter(|cwd| !cwd.is_empty()) {
                    Some(cwd) => Some(adopted(&home, claude_session_id, cwd, None, cx).await),
                    None => {
                        let _ = this.update(cx, |this, _| {
                            this.log.warn(
                                "found session has no folder",
                                Some(json!({ "claudeSession": claude_session_id })),
                            );
                        });
                        None
                    }
                }
            }),
            ListItemRef::Running {
                pid, claude_session_id, ..
            } => cx.spawn(async move |this, cx| {
                let look = home.clone();
                let entry = cx
                    .background_executor()
                    .spawn(async move {
                        read_registry_entry(&look, pid)
                            .filter(|entry| is_running_entry(entry, &is_alive, &process_start))
                    })
                    .await;
                let Some(entry) = entry.filter(|entry| {
                    entry.session_id == claude_session_id && entry.cwd.as_deref().is_some_and(|cwd| !cwd.is_empty())
                }) else {
                    let _ = this.update(cx, |this, _| {
                        this.log.info(
                            "running session is gone, nothing to take over",
                            Some(json!({ "pid": pid })),
                        );
                    });
                    return None;
                };
                let cwd = entry.cwd.clone().unwrap_or_default();
                if !confirmed {
                    let label = entry.name.clone().unwrap_or_else(|| folder_name(&cwd).to_owned());
                    let question = this
                        .update(cx, |this, cx| this.ask_takeover(&window_id, &[label], cx))
                        .ok()?;
                    if !question.await {
                        return None;
                    }
                }
                let stopping = this
                    .update(cx, |this, cx| {
                        this.log.info(
                            "taking over session from another terminal",
                            Some(json!({ "pid": pid, "claudeSession": entry.session_id })),
                        );
                        this.stop_outside(pid, cx)
                    })
                    .ok()?;
                if !stopping.await {
                    let _ = this.update(cx, |this, cx| {
                        let message = t(this.locale(), "takeover.failed", &[]).unwrap_or_default();
                        this.show_error(&window_id, message, cx);
                    });
                    return None;
                }
                // A name the user gave it survives; one Claude made up is made up again.
                let name = entry
                    .name
                    .clone()
                    .filter(|_| entry.name_source.as_deref() == Some("user"));
                Some(adopted(&home, entry.session_id, cwd, name, cx).await)
            }),
        }
    }

    /// Opens a list entry in a window, at `target`; a session shown somewhere already is just
    /// brought up.
    pub fn open_session(
        &mut self,
        window_id: &str,
        item: &ListItemRef,
        target: Option<DropTarget>,
        cx: &mut Context<Self>,
    ) {
        let record = self.record_for(window_id, item, false, cx);
        let window_id = window_id.to_owned();
        cx.spawn(async move |this, cx| {
            let Some(record) = record.await else { return };
            let _ = this.update(cx, |this, cx| {
                if this.window_of(&record.id).is_some() {
                    return this.focus_session(&record.id, cx);
                }
                let record = SessionRecord {
                    lifecycle: Lifecycle::Open,
                    ..record
                };
                this.place_and_resume(&window_id, record, target.as_ref(), cx);
            });
        })
        .detach();
    }

    fn place_and_resume(
        &mut self,
        window_id: &str,
        record: SessionRecord,
        target: Option<&DropTarget>,
        cx: &mut Context<Self>,
    ) {
        if self.find_window(window_id).is_none() {
            return;
        }
        let id = record.id.clone();
        self.state.sessions.insert(id.clone(), record);
        self.mutate_window(window_id, cx, |window| put_node(window, tile(id.clone()), target));
        self.log.info(
            "session opened from the list",
            Some(json!({ "session": id, "window": window_id })),
        );
        self.start_or_take_over(&id, LaunchMode::Resume, Some(window_id.to_owned()), cx);
        cx.emit(AppEvent::FocusTile {
            window: window_id.to_owned(),
            session: id,
        });
    }

    /// Opens several list entries at once: one after the other (at `target`, each below the one
    /// before), or together in a new group, in this window or a new one. Sessions running
    /// elsewhere are taken over after one question for all of them. Opening sessions that are
    /// shown already just brings them up; dropping them somewhere or grouping them moves them
    /// there.
    pub fn open_sessions(
        &mut self,
        window_id: &str,
        items: Vec<ListItemRef>,
        mode: OpenMode,
        target: Option<DropTarget>,
        cx: &mut Context<Self>,
    ) {
        let window_id = window_id.to_owned();
        let home = self.claude_home.clone();
        cx.spawn(async move |this, cx| {
            let mut wanted = items;
            let running: Vec<u32> = wanted
                .iter()
                .filter_map(|item| match item {
                    ListItemRef::Running { pid, .. } => Some(*pid),
                    _ => None,
                })
                .collect();
            if !running.is_empty() {
                let labels = cx
                    .background_executor()
                    .spawn(async move {
                        running
                            .iter()
                            .map(|pid| {
                                let entry = read_registry_entry(&home, *pid);
                                entry
                                    .as_ref()
                                    .and_then(|entry| entry.name.clone())
                                    .or_else(|| {
                                        entry
                                            .and_then(|entry| entry.cwd)
                                            .map(|cwd| folder_name(&cwd).to_owned())
                                    })
                                    .unwrap_or_else(|| pid.to_string())
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;
                let Ok(question) = this.update(cx, |this, cx| this.ask_takeover(&window_id, &labels, cx)) else {
                    return;
                };
                if !question.await {
                    wanted.retain(|item| !matches!(item, ListItemRef::Running { .. }));
                }
            }
            let mut records: Vec<SessionRecord> = Vec::new();
            for item in &wanted {
                let Ok(record) = this.update(cx, |this, cx| this.record_for(&window_id, item, true, cx)) else {
                    return;
                };
                if let Some(record) = record.await
                    && !records.iter().any(|known| known.id == record.id)
                {
                    records.push(record);
                }
            }
            if records.is_empty() {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.place_records(&window_id, records, mode, target.as_ref(), cx)
            });
        })
        .detach();
    }

    fn place_records(
        &mut self,
        window_id: &str,
        records: Vec<SessionRecord>,
        mode: OpenMode,
        target: Option<&DropTarget>,
        cx: &mut Context<Self>,
    ) {
        if self.find_window(window_id).is_none() {
            return;
        }
        let mode_name = match mode {
            OpenMode::Open => "open",
            OpenMode::Group => "group",
            OpenMode::GroupInNewWindow => "groupInNewWindow",
        };
        self.log.info(
            "opening sessions from the list",
            Some(json!({ "sessions": records.len(), "mode": mode_name, "at": target.map_or("end", |_| "target") })),
        );
        if mode == OpenMode::Open && target.is_none() {
            for record in records {
                if self.window_of(&record.id).is_some() {
                    self.focus_session(&record.id, cx);
                } else {
                    let record = SessionRecord {
                        lifecycle: Lifecycle::Open,
                        ..record
                    };
                    self.place_and_resume(window_id, record, None, cx);
                }
            }
            return;
        }
        let into = if mode == OpenMode::GroupInNewWindow {
            let near = self.find_window(window_id).and_then(|window| window.bounds);
            self.new_window(near, cx)
        } else {
            window_id.to_owned()
        };
        let ids: Vec<String> = records.iter().map(|record| record.id.clone()).collect();
        for record in records {
            let current = self.state.sessions.get(&record.id).cloned().unwrap_or(record);
            self.state.sessions.insert(
                current.id.clone(),
                SessionRecord {
                    lifecycle: Lifecycle::Open,
                    ..current
                },
            );
        }
        // Out of wherever they are shown now; a window left empty by that closes only after they
        // have landed.
        let left: HashSet<String> = ids
            .iter()
            .filter_map(|id| self.detach(id, cx))
            .filter(|window| *window != into)
            .collect();
        if mode == OpenMode::Open {
            let mut next = target.cloned();
            for id in &ids {
                let at = next.clone();
                self.mutate_window(&into, cx, |window| put_node(window, tile(id.clone()), at.as_ref()));
                next = Some(DropTarget {
                    key: tile_key(id),
                    side: reframe_core::state::Side::Bottom,
                });
            }
        } else {
            let mut child: Option<LayoutNode> = None;
            for id in &ids {
                child = Some(append_node(child.as_ref(), tile(id.clone())));
            }
            let (name, color) = self.new_group_look();
            let group = GroupNode {
                id: Uuid::new_v4().to_string(),
                name: name.clone(),
                color,
                child: Box::new(child.unwrap_or_else(|| tile(String::new()))),
            };
            self.mutate_window(&into, cx, |window| put_node(window, LayoutNode::Group(group), target));
            self.log.info(
                "group created from the list",
                Some(json!({ "sessions": ids.len(), "name": name })),
            );
        }
        for id in &ids {
            self.start_or_take_over(id, LaunchMode::Resume, Some(into.clone()), cx);
        }
        for window in left {
            self.close_if_empty(&window, cx);
        }
        if let Some(first) = ids.first() {
            cx.emit(AppEvent::FocusTile {
                window: into,
                session: first.clone(),
            });
        }
    }

    /// Takes a session's tile out of the window showing it (a collapsed one is put back first);
    /// that window's id.
    fn detach(&mut self, session_id: &str, cx: &mut Context<Self>) -> Option<String> {
        let window = self.window_of(session_id)?.clone();
        let mut next = window.clone();
        for entry in &window.dock {
            match entry {
                DockEntry::Tile { session_id: id, .. } if id == session_id => {
                    next = apply_layout_action(&next, &LayoutAction::TileExpand { session_id: id.clone() });
                }
                DockEntry::Group { node, .. } if leaves(Some(&node.child)).iter().any(|id| id == session_id) => {
                    next = apply_layout_action(
                        &next,
                        &LayoutAction::GroupExpand {
                            group_id: node.id.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        let rest = take_node(&next, &tile_key(session_id)).win;
        self.mutate_window(&window.id, cx, |_| rest.clone());
        Some(window.id)
    }

    // ---------- workspaces: windows saved under a name ----------

    /// Saves the window as a new workspace (a snapshot; later changes stay out until it is
    /// updated).
    pub fn save_workspace(&mut self, window_id: &str, name: &str, cx: &mut Context<Self>) {
        let Some(window) = self.find_window(window_id) else {
            return;
        };
        let trimmed: String = name.trim().chars().take(80).collect();
        let name = if trimmed.is_empty() {
            let n = (self.state.workspaces.len() + 1).to_string();
            t(self.locale(), "workspace.defaultName", &[("n", &n)]).unwrap_or_default()
        } else {
            trimmed
        };
        let workspace = snapshot_window(window, &self.state.sessions, Uuid::new_v4().to_string(), name, now_ms());
        let (id, count) = (workspace.id.clone(), workspace.sessions.len());
        self.state.workspaces.push(workspace);
        if let Some(window) = self.state.windows.iter_mut().find(|window| window.id == window_id) {
            window.workspace_id = Some(id.clone());
        }
        self.save(cx);
        self.log
            .info("workspace saved", Some(json!({ "workspace": id, "sessions": count })));
        cx.emit(AppEvent::Window(window_id.to_owned()));
        cx.emit(AppEvent::Workspaces);
    }

    /// Saves the window again over the workspace it was saved as or opened from.
    pub fn update_workspace(&mut self, window_id: &str, cx: &mut Context<Self>) {
        let Some(window) = self.find_window(window_id) else {
            return;
        };
        let Some(old) = self.workspace_of(window) else {
            return;
        };
        let workspace = snapshot_window(window, &self.state.sessions, old.id.clone(), old.name.clone(), now_ms());
        let (id, count) = (workspace.id.clone(), workspace.sessions.len());
        if let Some(slot) = self.state.workspaces.iter_mut().find(|saved| saved.id == id) {
            *slot = workspace;
        }
        self.save(cx);
        self.log
            .info("workspace updated", Some(json!({ "workspace": id, "sessions": count })));
        cx.emit(AppEvent::Workspaces);
    }

    /// Opens a workspace as a window of its own; if its window is open already, that one comes
    /// up. Sessions shown elsewhere move into it, the others resume; sessions that left the list
    /// since come back from the snapshot.
    pub fn open_workspace(&mut self, workspace_id: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self
            .state
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .cloned()
        else {
            return;
        };
        if let Some(open) = self.window_of_workspace(workspace_id) {
            cx.emit(AppEvent::ShowWindow(open.id.clone()));
            return;
        }
        // A window that was the workspace's but shows none of its sessions any more is just a
        // window now.
        for window in &mut self.state.windows {
            if window.workspace_id.as_deref() == Some(workspace_id) {
                window.workspace_id = None;
            }
        }
        let mut ids: HashMap<String, String> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for saved in &workspace.sessions {
            let current = self.state.sessions.get(&saved.id).cloned().or_else(|| {
                self.state
                    .sessions
                    .values()
                    .find(|record| record.claude_session_id == saved.claude_session_id)
                    .cloned()
            });
            let record = current.unwrap_or_else(|| saved.clone());
            ids.insert(saved.id.clone(), record.id.clone());
            order.push(record.id.clone());
            self.state.sessions.insert(
                record.id.clone(),
                SessionRecord {
                    lifecycle: Lifecycle::Open,
                    ..record
                },
            );
        }
        let left: HashSet<String> = order.iter().filter_map(|id| self.detach(id, cx)).collect();
        let in_use: HashSet<String> = self
            .state
            .windows
            .iter()
            .flat_map(window_groups)
            .map(|group| group.id.clone())
            .collect();
        let window = window_from_workspace(&workspace, &Uuid::new_v4().to_string(), &ids, &in_use, || {
            Uuid::new_v4().to_string()
        });
        let window_id = window.id.clone();
        self.state.windows.push(window);
        self.save(cx);
        cx.emit(AppEvent::WindowOpened(window_id.clone()));
        for id in &order {
            self.start_or_take_over(id, LaunchMode::Resume, Some(window_id.clone()), cx);
        }
        for window in left {
            self.close_if_empty(&window, cx);
        }
        self.log.info(
            "workspace opened",
            Some(json!({ "workspace": workspace_id, "window": window_id, "sessions": order.len() })),
        );
        cx.emit(AppEvent::Workspaces);
    }

    pub fn rename_workspace(&mut self, workspace_id: &str, name: &str, cx: &mut Context<Self>) {
        let trimmed: String = name.trim().chars().take(80).collect();
        let Some(workspace) = self
            .state
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
        else {
            return;
        };
        if trimmed.is_empty() {
            return;
        }
        workspace.name = trimmed;
        self.save(cx);
        cx.emit(AppEvent::Workspaces);
    }

    /// Forgets a saved workspace (after asking in that window); a window opened from it stays,
    /// as a plain window.
    pub fn delete_workspace(&mut self, window_id: &str, workspace_id: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self
            .state
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
        else {
            return;
        };
        let locale = self.locale();
        let text = |key: &str, params: &[(&str, &str)]| t(locale, key, params).unwrap_or_default();
        let question = Question {
            title: text("workspace.delete", &[]),
            message: text("workspace.deleteQuestion", &[("name", &workspace.name)]),
            detail: Some(text("workspace.deleteDetail", &[])),
            confirm: text("workspace.delete", &[]),
            cancel: Some(text("dialog.cancel", &[])),
            danger: true,
        };
        let answer = window::ask(window_id, question, cx);
        let workspace_id = workspace_id.to_owned();
        cx.spawn(async move |this, cx| {
            if !answer.await.unwrap_or(false) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.state.workspaces.retain(|workspace| workspace.id != workspace_id);
                let mut changed = Vec::new();
                for window in &mut this.state.windows {
                    if window.workspace_id.as_deref() == Some(workspace_id.as_str()) {
                        window.workspace_id = None;
                        changed.push(window.id.clone());
                    }
                }
                this.save(cx);
                for id in changed {
                    cx.emit(AppEvent::Window(id));
                }
                this.log
                    .info("workspace deleted", Some(json!({ "workspace": workspace_id })));
                cx.emit(AppEvent::Workspaces);
            });
        })
        .detach();
    }

    /// The saved workspaces for menus: id, name, and whether its window is open.
    pub fn workspace_items(&self) -> Vec<(String, String, bool)> {
        self.state
            .workspaces
            .iter()
            .map(|workspace| {
                let open = self.window_of_workspace(&workspace.id).is_some();
                (workspace.id.clone(), workspace.name.clone(), open)
            })
            .collect()
    }
}

/// A new record for a Claude Code session started outside reframe. `home`: where its Claude Code
/// keeps transcripts.
async fn adopted(
    home: &std::path::Path,
    claude_session_id: String,
    cwd: String,
    name: Option<String>,
    cx: &mut AsyncApp,
) -> SessionRecord {
    // Without a transcript (no prompt yet) it starts fresh under the same id instead of failing
    // to resume. Its last use is when its transcript was last written: opening it here is not
    // using it.
    let (home, id): (PathBuf, String) = (home.to_owned(), claude_session_id.clone());
    let modified = cx
        .background_executor()
        .spawn(async move { transcript_modified(&home, &id) })
        .await;
    let now = now_ms();
    SessionRecord {
        id: Uuid::new_v4().to_string(),
        claude_session_id,
        cwd,
        name,
        claude_name: None,
        launch: LaunchOptions::default(),
        lifecycle: Lifecycle::Open,
        created_at: now,
        last_active_at: modified.unwrap_or(now),
        pid: None,
        linux_pid: None,
        used: Some(modified.is_some()),
    }
}
