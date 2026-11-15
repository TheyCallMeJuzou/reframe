//! Follows title-bar drags of other programs' windows. When a terminal window that hosts a claude
//! running outside reframe is dragged over one of reframe's windows, that window shows where it
//! would go; on release, the claudes found in that terminal window are offered for taking over.
//! A claude in WSL is found by the wsl.exe of its terminal tab, whose console is the tab's.
//!
//! Dragging a tab out of Windows Terminal is no window move: the terminal only opens a new window
//! for the tab where it was released. A terminal window that appears over one of reframe's
//! windows, holding a claude that already ran before the window existed, counts as such a drop
//! too.
//!
//! Ported from the Electron version's `src/main/foreign.ts`. (Its pointer reports during a tab
//! drag over reframe, for showing where the tab would go, need a drop target that takes drags
//! without data, which GPUI's is not: that part is left out.)

use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use gpui_kit::*;
use reframe_core::claude::registry::RegistryEntry;
use reframe_core::foreign_match::{is_terminal_host, pids_for_drop, pids_in_window};
use serde_json::json;

use crate::app::Controller;
use crate::console::{console_owners, image_name};
use crate::win::{self, WindowEvent};
use crate::window;

/// A claude running outside reframe (from Claude Code's session registry).
#[derive(Clone, Debug)]
pub struct OutsideClaude {
    pub entry: RegistryEntry,
    /// Its WSL distribution; none for a claude on Windows itself.
    pub distro: Option<String>,
    /// The Windows process whose console is the claude's terminal: the claude, or in WSL its
    /// terminal's wsl.exe.
    pub console_pid: u32,
}

/// A terminal window with claudes, let go over one of reframe's windows.
#[derive(Clone, Debug)]
pub struct ForeignDrop {
    pub hwnd: isize,
    pub title: String,
    pub window_id: String,
    /// Where in that window (logical).
    pub at: Point<Pixels>,
    pub sessions: Vec<OutsideClaude>,
}

const TRACK: Duration = Duration::from_millis(50);
const REPROBE: Duration = Duration::from_millis(700);
/// How long after its creation a shown window counts as new.
const NEW_WINDOW: Duration = Duration::from_millis(10_000);
/// When to look into a new terminal window: the moved tab's console may name its window only
/// after a moment.
const APPEARED_PROBES_MS: [u64; 5] = [0, 300, 800, 1500, 2500];

struct Drag {
    hwnd: isize,
    /// Sessions found in the window so far; `None` until the first lookup is done.
    sessions: Option<Vec<OutsideClaude>>,
    probing: bool,
    last_probe: Instant,
    /// The window of reframe that shows where it would go.
    hovered: Option<String>,
    /// Follows the pointer once it is known that claudes run outside reframe.
    tracking: Option<Task<()>>,
}

pub struct ForeignWatcher {
    controller: Entity<Controller>,
    drag: Option<Drag>,
    created: HashMap<isize, Instant>,
    hosts: HashMap<u32, Option<String>>,
    _events: Task<()>,
}

struct Watcher(Entity<ForeignWatcher>);

impl Global for Watcher {}

/// The watcher, for tests to drive.
pub fn watcher(cx: &App) -> Option<Entity<ForeignWatcher>> {
    cx.try_global::<Watcher>().map(|watcher| watcher.0.clone())
}

/// Starts following other programs' windows.
pub fn start(controller: &Entity<Controller>, cx: &mut App) {
    let (sender, mut events) = unbounded();
    if !win::watch_windows(sender) {
        controller.read(cx).log().warn("cannot follow window drags", None);
    }
    let controller = controller.clone();
    let watcher = cx.new(|cx| ForeignWatcher {
        controller,
        drag: None,
        created: HashMap::new(),
        hosts: HashMap::new(),
        _events: cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |this: &mut ForeignWatcher, cx| this.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        }),
    });
    cx.set_global(Watcher(watcher));
}

impl ForeignWatcher {
    fn log(&self, cx: &App) -> reframe_core::log::Logger {
        self.controller.read(cx).log().clone()
    }

    fn on_event(&mut self, event: WindowEvent, cx: &mut Context<Self>) {
        match event {
            WindowEvent::MoveStarted(hwnd) => self.begin(hwnd, cx),
            WindowEvent::MoveEnded(hwnd) => self.end(hwnd, cx),
            WindowEvent::Created(hwnd) => {
                self.created.retain(|_, at| at.elapsed() <= NEW_WINDOW);
                self.created.insert(hwnd, Instant::now());
            }
            WindowEvent::Shown(hwnd) => {
                if self.created.remove(&hwnd).is_some_and(|at| at.elapsed() <= NEW_WINDOW) {
                    self.appeared(hwnd, None, cx);
                }
            }
        }
    }

    fn host_of(&mut self, hwnd: isize) -> Option<String> {
        let pid = win::window_process(hwnd);
        if self.hosts.len() > 500 {
            self.hosts.clear();
        }
        self.hosts.entry(pid).or_insert_with(|| image_name(pid)).clone()
    }

    /// Claude sessions whose console lives in terminal window `hwnd` (`known`: the claudes
    /// outside reframe, if they were just looked up).
    pub fn sessions_in(
        &self,
        hwnd: isize,
        known: Option<Vec<OutsideClaude>>,
        cx: &mut Context<Self>,
    ) -> Task<Vec<OutsideClaude>> {
        let external = match known {
            Some(known) => Task::ready(known),
            None => self
                .controller
                .update(cx, |controller, cx| controller.outside_claudes(cx)),
        };
        let log = self.log(cx);
        cx.background_spawn(async move {
            let external = external.await;
            if external.is_empty() {
                return Vec::new();
            }
            let mut pids: Vec<u32> = external.iter().map(|claude| claude.console_pid).collect();
            pids.sort_unstable();
            pids.dedup();
            let owners = match console_owners(&pids) {
                Ok(owners) => owners,
                Err(error) => {
                    log.warn(
                        "could not map consoles to terminal windows",
                        Some(json!({ "error": error })),
                    );
                    return Vec::new();
                }
            };
            let found = pids_for_drop(hwnd, &owners, &win::is_window);
            if !found.is_empty() && pids_in_window(hwnd, &owners).is_empty() {
                log.info(
                    "matched a torn-out terminal tab by its vanished window",
                    Some(json!({ "pids": found })),
                );
            }
            external
                .into_iter()
                .filter(|claude| found.contains(&claude.console_pid))
                .collect()
        })
    }

    fn begin(&mut self, hwnd: isize, cx: &mut Context<Self>) {
        self.cancel(cx);
        // Moving windows is frequent: only terminals, and only while some claude runs outside
        // reframe, cost more.
        let host = self.host_of(hwnd);
        if !is_terminal_host(host.as_deref()) {
            return;
        }
        self.drag = Some(Drag {
            hwnd,
            sessions: None,
            probing: false,
            last_probe: Instant::now(),
            hovered: None,
            tracking: None,
        });
        let external = self
            .controller
            .update(cx, |controller, cx| controller.outside_claudes(cx));
        cx.spawn(async move |this, cx| {
            let external = external.await;
            let _ = this.update(cx, |this, cx| {
                // Released (or another drag began) meanwhile: end() looked for itself.
                if this.drag.as_ref().is_none_or(|drag| drag.hwnd != hwnd) {
                    return;
                }
                if external.is_empty() {
                    this.drag = None;
                    return;
                }
                this.log(cx).info(
                    "terminal window drag started",
                    Some(json!({ "host": host, "claudesElsewhere": external.len() })),
                );
                let tracking = cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(TRACK).await;
                        if this.update(cx, |this, cx| this.track(cx)).is_err() {
                            break;
                        }
                    }
                });
                if let Some(drag) = &mut this.drag {
                    drag.tracking = Some(tracking);
                }
                this.probe(Some(external), cx);
            });
        })
        .detach();
    }

    /// Looks again which claudes the dragged window holds.
    fn probe(&mut self, known: Option<Vec<OutsideClaude>>, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.as_mut().filter(|drag| !drag.probing) else {
            return;
        };
        drag.probing = true;
        drag.last_probe = Instant::now();
        let hwnd = drag.hwnd;
        let found = self.sessions_in(hwnd, known, cx);
        cx.spawn(async move |this, cx| {
            let found = found.await;
            let _ = this.update(cx, |this, _| {
                if let Some(drag) = this.drag.as_mut().filter(|drag| drag.hwnd == hwnd) {
                    drag.sessions = Some(found);
                    drag.probing = false;
                }
            });
        })
        .detach();
    }

    fn track(&mut self, cx: &mut Context<Self>) {
        let (x, y) = win::cursor();
        let over = window::window_at_physical(x, y, cx);
        let Some(drag) = self.drag.as_mut() else { return };
        if drag.sessions.as_ref().is_none_or(Vec::is_empty) {
            // Windows Terminal may move a torn-out tab's console to its new window only after a
            // moment: ask again.
            if over.is_some() && drag.sessions.is_some() && drag.last_probe.elapsed() > REPROBE {
                self.probe(None, cx);
            }
            return;
        }
        let over_id = over.as_ref().map(|(id, _)| id.clone());
        if let Some(previous) = drag
            .hovered
            .take()
            .filter(|previous| Some(previous) != over_id.as_ref())
            && let Some(shell) = window::shell(&previous, cx)
        {
            shell.update(cx, |shell, cx| shell.set_foreign_hover(None, cx));
        }
        drag.hovered = over_id;
        if let Some((id, at)) = over
            && let Some(shell) = window::shell(&id, cx)
        {
            shell.update(cx, |shell, cx| shell.set_foreign_hover(Some(at), cx));
        }
    }

    fn end(&mut self, hwnd: isize, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take().filter(|drag| drag.hwnd == hwnd) else {
            return;
        };
        let tracked = drag.tracking.is_some();
        self.clear_hover(drag.hovered.as_deref(), cx);
        let (x, y) = win::cursor();
        let over = window::window_at_physical(x, y, cx);
        let known = drag.sessions.clone();
        let found = match known {
            Some(found) => Task::ready(found),
            None => self.sessions_in(hwnd, None, cx),
        };
        cx.spawn(async move |this, cx| {
            let mut sessions = found.await;
            // Dropped on reframe with nothing found yet: one more look, now that the window has
            // settled.
            if over.is_some() && sessions.is_empty() {
                let Ok(again) = this.update(cx, |this, cx| this.sessions_in(hwnd, None, cx)) else {
                    return;
                };
                sessions = again.await;
            }
            let _ = this.update(cx, |this, cx| {
                // A drag released before it was known whether claudes run outside reframe is
                // worth a line only over reframe.
                if tracked || over.is_some() {
                    this.log(cx).info(
                        "terminal window drag ended",
                        Some(json!({ "overReframe": over.is_some(), "sessions": sessions.len() })),
                    );
                }
                let Some((window_id, at)) = over.filter(|_| !sessions.is_empty()) else {
                    return;
                };
                let drop = ForeignDrop {
                    hwnd,
                    title: win::window_title(hwnd),
                    window_id,
                    at,
                    sessions,
                };
                this.controller
                    .update(cx, |controller, cx| controller.take_over_window(drop, cx));
            });
        })
        .detach();
    }

    fn clear_hover(&self, hovered: Option<&str>, cx: &mut App) {
        if let Some(shell) = hovered.and_then(|id| window::shell(id, cx)) {
            shell.update(cx, |shell, cx| shell.set_foreign_hover(None, cx));
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.drag.take() {
            self.clear_hover(drag.hovered.as_deref(), cx);
        }
    }

    /// A new terminal window was shown: when it holds a claude that ran before and appeared over
    /// one of reframe's windows, it is a tab torn out over reframe. `point` (tests, in screen
    /// pixels) stands in for the pointer.
    pub fn appeared(&mut self, hwnd: isize, point: Option<(i32, i32)>, cx: &mut Context<Self>) {
        let host = self.host_of(hwnd);
        if !is_terminal_host(host.as_deref()) {
            return;
        }
        // Helper windows of shells, never seen.
        if win::window_rect(hwnd).is_none_or(|(.., width, height)| width == 0 || height == 0) {
            return;
        }
        let shown_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |since| since.as_millis() as f64);
        let (x, y) = point.unwrap_or_else(win::cursor);
        let over = window::window_at_physical(x, y, cx);
        let external = self
            .controller
            .update(cx, |controller, cx| controller.outside_claudes(cx));
        cx.spawn(async move |this, cx| {
            // Only claudes that already ran before the window appeared: one started in a new
            // window was not dropped here.
            let external: Vec<OutsideClaude> = external
                .await
                .into_iter()
                .filter(|claude| claude.entry.started_at.is_none_or(|started| started < shown_at))
                .collect();
            if external.is_empty() {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.log(cx).info(
                    "new terminal window appeared",
                    Some(json!({
                        "host": host,
                        "overReframe": over.is_some(),
                        "point": [x, y],
                        "pointFrom": if point.is_some() { "test" } else { "cursor" },
                    })),
                );
            });
            let Some((window_id, at)) = over else { return };
            let mut waited = 0;
            let mut sessions = Vec::new();
            for delay in APPEARED_PROBES_MS {
                cx.background_executor()
                    .timer(Duration::from_millis(delay - waited))
                    .await;
                waited = delay;
                if !win::is_window(hwnd) {
                    return;
                }
                let Ok(found) = this.update(cx, |this, cx| this.sessions_in(hwnd, Some(external.clone()), cx)) else {
                    return;
                };
                sessions = found.await;
                if !sessions.is_empty() {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.log(cx).info(
                    "claudes in the new terminal window",
                    Some(json!({ "sessions": sessions.len() })),
                );
                if sessions.is_empty() {
                    return;
                }
                let drop = ForeignDrop {
                    hwnd,
                    title: win::window_title(hwnd),
                    window_id,
                    at,
                    sessions,
                };
                this.controller
                    .update(cx, |controller, cx| controller.take_over_window(drop, cx));
            });
        })
        .detach();
    }
}
