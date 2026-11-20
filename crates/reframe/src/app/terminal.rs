//! Terminal windows dragged onto reframe: which claudes run outside reframe and in which terminal,
//! taking over those of a dropped terminal window, and closing the tab they leave behind when
//! nothing but a shell is left there.
//!
//! Ported from the Electron version's `src/main/app.ts` (`outsideClaudes`, `takeOverWindow`,
//! `closeIdleWslTab`, `closeIdleConsole`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::*;
use reframe_core::claude::registry::{RegistryEntry, is_running_entry, read_registry_entry};
use reframe_core::discovery::{RunningExclusion, list_running};
use reframe_core::foreign_match::{active_by_title, is_shell, only_shells};
use reframe_core::layout::{DropTarget, tile_key};
use reframe_core::log::Logger;
use reframe_core::state::Side;
use reframe_core::wsl::wsl_windows_path;
use serde_json::json;

use super::{AppEvent, Controller, open::adopted};
use crate::console::{processes_in_console, type_into_console};
use crate::foreign::{ForeignDrop, OutsideClaude};
use crate::process::{is_alive, process_start};
use crate::window::{self, TerminalCandidate};
use crate::wsl::WslDistros;

/// Whether a program is one of those that only start the real wsl.exe or host consoles.
fn is_console_plumbing(name: Option<&str>) -> bool {
    name.is_some_and(|name| ["wsl.exe", "conhost.exe", "openconsole.exe"].contains(&name.to_lowercase().as_str()))
}

/// What closing a terminal tab needs to know, from before its claude was ended.
struct TabToClose {
    distro: Option<String>,
    console_pid: u32,
    /// The shells in the distribution that claude was typed into, innermost first.
    linux_shells: Vec<u32>,
    /// A shell on Windows in that console (in WSL: one that started wsl.exe).
    shell: Option<u32>,
}

impl Controller {
    /// Claudes running outside reframe (with a known folder), and the Windows process whose
    /// console is their terminal: on Windows the claude itself, in a running WSL distribution
    /// the wsl.exe of its terminal (one whose terminal cannot be told is left out).
    pub fn outside_claudes(&self, cx: &mut Context<Self>) -> Task<Vec<OutsideClaude>> {
        let own_ids: HashSet<String> = self
            .state
            .sessions
            .values()
            .map(|record| record.claude_session_id.clone())
            .collect();
        let own = self.sessions.read(cx).own_pids();
        let own_linux: HashSet<u32> = self
            .state
            .sessions
            .values()
            .filter_map(|record| record.linux_pid)
            .collect();
        let (home, wsl) = (self.claude_home.clone(), self.wsl.clone());
        cx.background_executor().spawn(async move {
            let exclusion = RunningExclusion {
                pids: own,
                session_ids: own_ids.clone(),
            };
            let alive = |entry: &RegistryEntry| is_running_entry(entry, &is_alive, &process_start);
            let with_folder = |entry: &RegistryEntry| entry.cwd.as_deref().is_some_and(|cwd| !cwd.is_empty());
            let mut found: Vec<OutsideClaude> = list_running(&home, &exclusion, &alive)
                .into_iter()
                .filter(with_folder)
                .map(|entry| OutsideClaude {
                    console_pid: entry.pid,
                    entry,
                    distro: None,
                })
                .collect();
            for distro in wsl.running(None) {
                let Some(info) = wsl.info(&distro).filter(|info| info.claude.is_some()) else {
                    continue;
                };
                let home = PathBuf::from(wsl_windows_path(&distro, &info.claude_home, &wsl.share));
                let exclusion = RunningExclusion {
                    pids: own_linux.clone(),
                    session_ids: own_ids.clone(),
                };
                let alive = |entry: &RegistryEntry| wsl.is_claude(&distro, entry.pid);
                for entry in list_running(&home, &exclusion, &alive).into_iter().filter(with_folder) {
                    if let Some(terminal) = wsl.terminal_of(&distro, &entry) {
                        found.push(OutsideClaude {
                            entry,
                            distro: Some(distro.clone()),
                            console_pid: terminal,
                        });
                    }
                }
            }
            found
        })
    }

    /// Asks (in the window it was dropped on) which sessions of a terminal window to take over;
    /// resumes them there; closes the terminal tabs they leave with nothing but a shell.
    pub fn take_over_window(&mut self, drop: ForeignDrop, cx: &mut Context<Self>) {
        // Some terminals report the end of a window move twice; one question per window is
        // enough.
        if !self.taking_over.insert(drop.hwnd) {
            self.log.info("repeated drop of the same terminal window ignored", None);
            return;
        }
        self.log.info(
            "terminal window dropped on reframe",
            Some(json!({ "title": drop.title, "sessions": drop.sessions.len() })),
        );
        cx.emit(AppEvent::ShowWindow(drop.window_id.clone()));
        let share = self.wsl.share.clone();
        let windows_path = |distro: Option<&str>, cwd: &str| match distro {
            Some(distro) => wsl_windows_path(distro, cwd, &share),
            None => cwd.to_owned(),
        };
        let names: Vec<Option<&str>> = drop
            .sessions
            .iter()
            .map(|claude| claude.entry.name.as_deref())
            .collect();
        let active = active_by_title(&drop.title, &names);
        let single = drop.sessions.len() == 1;
        let candidates: Vec<TerminalCandidate> = drop
            .sessions
            .iter()
            .enumerate()
            .map(|(index, claude)| TerminalCandidate {
                claude_session_id: claude.entry.session_id.clone(),
                cwd: windows_path(
                    claude.distro.as_deref(),
                    claude.entry.cwd.as_deref().unwrap_or_default(),
                ),
                name: claude.entry.name.clone(),
                preselected: single || active == Some(index),
            })
            .collect();
        let answer = window::ask_terminal(&drop.window_id, candidates, drop.at, cx);
        let hwnd = drop.hwnd;
        cx.spawn(async move |this, cx| {
            let answer = answer.await.ok().flatten();
            let _ = Self::take_over_chosen(&this, drop, answer, cx).await;
            // Let a second report of the same release go by first.
            cx.background_executor().timer(Duration::from_millis(2000)).await;
            let _ = this.update(cx, |this, _| this.taking_over.remove(&hwnd));
        })
        .detach();
    }

    async fn take_over_chosen(
        this: &WeakEntity<Self>,
        drop: ForeignDrop,
        answer: Option<(Vec<String>, Option<DropTarget>)>,
        cx: &mut AsyncApp,
    ) -> Option<()> {
        let Some((ids, target)) = answer else {
            this.update(cx, |this, _| this.log.info("terminal window takeover cancelled", None))
                .ok()?;
            return None;
        };
        let chosen: Vec<OutsideClaude> = drop
            .sessions
            .into_iter()
            .filter(|claude| ids.contains(&claude.entry.session_id))
            .collect();
        let (log, wsl, home, share) = this
            .update(cx, |this, _| {
                (
                    this.log.clone(),
                    this.wsl.clone(),
                    this.claude_home.clone(),
                    this.wsl.share.clone(),
                )
            })
            .ok()?;
        log.info(
            "terminal window takeover confirmed",
            Some(json!({
                "sessions": chosen.len(),
                "wsl": chosen.iter().filter(|claude| claude.distro.is_some()).count(),
                "target": if target.is_some() { "where dropped" } else { "end of the window" },
            })),
        );
        let mut target = target;
        let mut tabs = Vec::new();
        for claude in chosen {
            let OutsideClaude {
                entry,
                distro,
                console_pid,
            } = claude;
            // Remember a shell in that console, to ask it afterwards what is left there. In WSL
            // that is one on Windows that started wsl.exe, if any (not wsl.exe itself, nor the
            // wsl.exe in System32 that starts the real one), and the shells in the distribution
            // that claude was typed into.
            let looked = {
                let (wsl, distro, home, entry) = (wsl.clone(), distro.clone(), home.clone(), entry.clone());
                cx.background_executor()
                    .spawn(async move {
                        let before = processes_in_console(console_pid);
                        let shell = before.as_ref().ok().and_then(|processes| {
                            processes
                                .iter()
                                .find(|process| {
                                    is_shell(process.name.as_deref())
                                        && !(distro.is_some()
                                            && (process.pid == console_pid
                                                || is_console_plumbing(process.name.as_deref())))
                                })
                                .map(|process| process.pid)
                        });
                        let linux_shells = distro
                            .as_ref()
                            .map(|distro| wsl.shells_of(distro, entry.pid))
                            .unwrap_or_default();
                        let home = match &distro {
                            Some(distro) => wsl
                                .info(distro)
                                .map(|info| PathBuf::from(wsl_windows_path(distro, &info.claude_home, &wsl.share))),
                            None => Some(home),
                        };
                        let latest = home.as_ref().and_then(|home| read_registry_entry(home, entry.pid));
                        (before, shell, linux_shells, home, latest)
                    })
                    .await
            };
            let (before, shell, linux_shells, source_home, latest) = looked;
            let Some(source_home) = source_home else { continue };
            let mut running = match &before {
                Ok(processes) => json!({
                    "processes": processes.iter().map(|process| process.name.clone()).collect::<Vec<_>>(),
                }),
                Err(error) => json!({ "error": error }),
            };
            if distro.is_some() {
                running["shellsInWsl"] = json!(linux_shells.len());
            }
            log.info("running in that terminal tab", Some(running));
            let latest = latest.unwrap_or_else(|| entry.clone());
            let stopping = this
                .update(cx, |this, cx| this.stop_outside(entry.pid, distro.clone(), cx))
                .ok()?;
            if !stopping.await {
                continue;
            }
            let name = latest
                .name
                .clone()
                .filter(|_| latest.name_source.as_deref() == Some("user"));
            let folder = latest.cwd.clone().or(entry.cwd.clone()).unwrap_or_default();
            let cwd = match &distro {
                Some(distro) => wsl_windows_path(distro, &folder, &share),
                None => folder,
            };
            let record = adopted(&source_home, latest.session_id.clone(), cwd, name, cx).await;
            let id = record.id.clone();
            this.update(cx, |this, cx| {
                this.place_and_resume(&drop.window_id, record, target.as_ref(), cx);
            })
            .ok()?;
            target = Some(DropTarget {
                key: tile_key(&id),
                side: Side::Right,
            });
            // Without a shell, the tab closed on its own together with claude.
            if distro.is_some() || shell.is_some() {
                tabs.push(TabToClose {
                    distro,
                    console_pid,
                    linux_shells,
                    shell,
                });
            }
        }
        cx.background_executor()
            .spawn(async move {
                for tab in tabs {
                    match &tab.distro {
                        Some(distro) => close_idle_wsl_tab(&wsl, &log, distro, &tab),
                        None => {
                            if let Some(shell) = tab.shell {
                                close_idle_console(&log, shell);
                            }
                        }
                    }
                }
            })
            .await;
        Some(())
    }
}

/// Closes a terminal tab (console) that has nothing but shells left, by typing "exit" as its user
/// would. Only that tab goes; other tabs of the same terminal window are never touched, and the
/// last one takes the window with it. Nested shells (bash started from PowerShell, say) get one
/// "exit" each. Blocks for seconds.
fn close_idle_console(log: &Logger, shell_pid: u32) {
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(500));
        // Nothing to attach to any more.
        let Ok(left) = processes_in_console(shell_pid) else {
            return log.info("terminal tab closed", None);
        };
        if !only_shells(&left) {
            let running: Vec<Option<String>> = left.into_iter().map(|process| process.name).collect();
            return log.info(
                "terminal tab stays open, something else runs there",
                Some(json!({ "running": running })),
            );
        }
        // Any shell of the tab will do: they share the console input, and the one in front reads
        // the "exit".
        let Some(shell) = left.iter().find(|process| {
            is_shell(process.name.as_deref())
                && !matches!(
                    process.name.as_deref().map(str::to_lowercase).as_deref(),
                    Some("conhost.exe" | "openconsole.exe")
                )
        }) else {
            return;
        };
        log.info(
            "only a shell is left in the terminal tab, typing exit",
            Some(json!({ "shell": shell.name })),
        );
        let _ = type_into_console(shell.pid, "exit\r");
    }
}

/// Closes the terminal tab of a claude taken over from WSL when nothing but shells is left there,
/// as for a tab on Windows: each shell in the distribution that claude was typed into gets an
/// "exit" (innermost first) once nothing runs in it, and nothing but shells besides wsl.exe on
/// Windows; the last one ends wsl.exe. A shell on Windows that started wsl.exe is
/// `close_idle_console`'s then. Blocks for seconds.
fn close_idle_wsl_tab(wsl: &Arc<WslDistros>, log: &Logger, distro: &str, tab: &TabToClose) {
    let wsl_pid = tab.console_pid;
    for &shell in &tab.linux_shells {
        std::thread::sleep(Duration::from_millis(500));
        if !wsl.idle(distro, shell) {
            return log.info(
                "terminal tab stays open, something else runs there in wsl",
                Some(json!({ "distro": distro })),
            );
        }
        let Ok(left) = processes_in_console(wsl_pid) else {
            return log.info("terminal tab closed", None);
        };
        let others: Vec<_> = left.into_iter().filter(|process| process.pid != wsl_pid).collect();
        if !only_shells(&others) {
            let running: Vec<Option<String>> = others.into_iter().map(|process| process.name).collect();
            return log.info(
                "terminal tab stays open, something else runs there",
                Some(json!({ "running": running })),
            );
        }
        log.info(
            "only a shell is left in the terminal tab, typing exit",
            Some(json!({ "shell": "wsl", "distro": distro })),
        );
        let _ = type_into_console(wsl_pid, "exit\r");
        for _ in 0..15 {
            if !wsl.alive(distro, shell) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let Some(windows_shell) = tab.shell else { return };
    for _ in 0..15 {
        if !is_alive(wsl_pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if !is_alive(wsl_pid) {
        close_idle_console(log, windows_shell);
    }
}
