//! Runs claude processes for sessions and tracks what they are doing. Status comes from two
//! sources: HTTP hooks (documented; prompt submitted, response finished, needs input) and Claude
//! Code's session registry file for the process (undocumented; immediate session id changes on
//! /clear).
//!
//! Ported from the Electron version's `src/main/sessions.ts`. The decisions are in
//! `reframe_core::session_logic`; this is the part with processes, timers and events. A session
//! in a WSL folder runs its claude in that distribution, through wsl.exe; the hooks cannot reach
//! reframe from there, so its registry entry (read over the share) is what tells its status.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::future::{FutureExt, Shared};
use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui_kit::*;
use reframe_core::claude::args::{LaunchMode, claude_args};
use reframe_core::claude::registry::{
    RegistryEntry, find_registry_entry, forget_registry_entry, is_running_entry, read_registry_entry, transcript_exists,
};
use reframe_core::claude::resolve::{ClaudeBinary, cmd_command_line};
use reframe_core::claude::settings::write_hook_settings;
use reframe_core::hooks::HookMessage;
use reframe_core::last_used::counts_as_use;
use reframe_core::log::Logger;
use reframe_core::session_logic::{
    IdentityPatch, NotifyKind, POLL_MS, RegistryMemory, RegistryPlace, SessionError, SessionStatus, exit_outcome,
    follow_registry, hook_effects, is_running, is_stale_entry, launch_mode,
};
use reframe_core::state::SessionRecord;
use reframe_core::wsl::{WslPath, parse_wsl_path, wsl_windows_path};
use serde_json::json;

use crate::process::{is_alive, kill_tree, process_start};
use crate::pty::{Launch, Pty};
use crate::terminal::{Terminal, TerminalEvent};
use crate::wsl::{SHARE_TIMEOUT, WslDistros};

/// A session running in WSL: its distribution, Claude's folder there (as Windows reaches it), and
/// its registry entry once found.
struct WslRuntime {
    distro: String,
    home: PathBuf,
    claude_session_id: String,
    linux_pid: Option<u32>,
    /// A read over the share is under way; the next tick skips it.
    reading: bool,
}

/// What a background job gives back, or `fallback` when it takes longer than `limit`.
async fn within<T: Send + 'static>(
    cx: &mut AsyncApp,
    limit: Duration,
    fallback: T,
    job: impl FnOnce() -> T + Send + 'static,
) -> T {
    use futures::FutureExt as _;
    let work = cx.background_executor().spawn(async move { job() }).fuse();
    let timer = cx.background_executor().timer(limit).fuse();
    futures::pin_mut!(work, timer);
    futures::select_biased! {
        value = work => value,
        () = timer => fallback,
    }
}

pub enum SessionEvent {
    /// Something visible changed (status, label, error).
    Changed(String),
    /// Persisted identity changed: claude session id (after /clear), folder (worktrees), pid, or
    /// first use.
    Identity(String, IdentityPatch),
    Notify(String, NotifyKind),
    /// The session was just used: a prompt sent, or Claude done or asking something.
    Used(String),
    /// The process ended on its own (not because reframe stopped it).
    Ended(String),
}

/// A claude run through cmd.exe (a launcher reframe does not know): its registry entry is under
/// its own process id, found by its session id.
struct Launched {
    claude_session_id: String,
    pid: Option<u32>,
}

struct Runtime {
    /// Which start of the session this is; events of an older process carry an older number.
    instance: u64,
    status: SessionStatus,
    mode: LaunchMode,
    started: Instant,
    /// The same moment by the clock, to compare with what a registry entry says.
    started_at_ms: f64,
    pid: Option<u32>,
    /// The process's start as the system tells it, to tell its registry entry from one left
    /// behind under its id.
    proc_start: Option<String>,
    /// A registry entry left behind by another process was seen (and logged) for it.
    stale_seen: bool,
    error: Option<SessionError>,
    registry: RegistryMemory,
    stopping: Option<Shared<Task<()>>>,
    exited: bool,
    launched: Option<Launched>,
    wsl: Option<WslRuntime>,
    terminal: Option<Entity<Terminal>>,
    _terminal_events: Option<Subscription>,
}

impl Runtime {
    fn new(instance: u64, status: SessionStatus, mode: LaunchMode) -> Self {
        Self {
            instance,
            status,
            mode,
            started: Instant::now(),
            started_at_ms: now_ms(),
            pid: None,
            proc_start: None,
            stale_seen: false,
            error: None,
            registry: RegistryMemory::default(),
            stopping: None,
            exited: false,
            launched: None,
            wsl: None,
            terminal: None,
            _terminal_events: None,
        }
    }
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}

pub struct SessionManagerDeps {
    pub claude_home: PathBuf,
    /// Where the per-session hook settings files go.
    pub settings_dir: PathBuf,
    pub hooks_port: u16,
    pub hooks_token: String,
    pub resolve_claude: Box<dyn Fn() -> Option<ClaudeBinary>>,
    pub wsl: Arc<WslDistros>,
    pub log: Logger,
}

pub struct SessionManager {
    deps: SessionManagerDeps,
    runtimes: HashMap<String, Runtime>,
    instances: u64,
    via_cmd_logged: bool,
    /// When a distribution not answering in time was last logged.
    slow_share_logged: Option<Instant>,
    _hooks: Task<()>,
    _poller: Task<()>,
}

impl EventEmitter<SessionEvent> for SessionManager {}

impl SessionManager {
    /// `hooks` yields what the hook server receives.
    pub fn new(deps: SessionManagerDeps, mut hooks: UnboundedReceiver<HookMessage>, cx: &mut Context<Self>) -> Self {
        let hook_task = cx.spawn(async move |this, cx| {
            while let Some(message) = hooks.next().await {
                if this.update(cx, |this, cx| this.on_hook(message, cx)).is_err() {
                    break;
                }
            }
        });
        let poller = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(POLL_MS)).await;
                if this.update(cx, |this, cx| this.poll_registry(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            deps,
            runtimes: HashMap::new(),
            instances: 0,
            via_cmd_logged: false,
            slow_share_logged: None,
            _hooks: hook_task,
            _poller: poller,
        }
    }

    pub fn status(&self, session_id: &str) -> Option<SessionStatus> {
        self.runtimes.get(session_id).map(|runtime| runtime.status)
    }

    pub fn error(&self, session_id: &str) -> Option<SessionError> {
        self.runtimes.get(session_id).and_then(|runtime| runtime.error)
    }

    /// The pty's process id of the session's current start.
    pub fn pid(&self, session_id: &str) -> Option<u32> {
        self.runtimes.get(session_id).and_then(|runtime| runtime.pid)
    }

    pub fn is_running(&self, session_id: &str) -> bool {
        self.status(session_id).is_some_and(is_running)
    }

    /// Running and staying so: not on its way out after it was ended (then it needs to be started
    /// again).
    pub fn keeps_running(&self, session_id: &str) -> bool {
        self.is_running(session_id)
            && self
                .runtimes
                .get(session_id)
                .is_some_and(|runtime| runtime.stopping.is_none())
    }

    /// Display name Claude gave the session (used when the user did not name it).
    pub fn registry_name(&self, session_id: &str) -> Option<&str> {
        self.runtimes
            .get(session_id)
            .and_then(|runtime| runtime.registry.name.as_deref())
    }

    /// The terminal of the session's current start, for views to draw.
    pub fn terminal(&self, session_id: &str) -> Option<Entity<Terminal>> {
        self.runtimes
            .get(session_id)
            .and_then(|runtime| runtime.terminal.clone())
    }

    /// The processes of reframe's sessions: the pty's, and claude's own once its entry was found
    /// (through cmd.exe they differ). What runs outside reframe is everything else.
    pub fn own_pids(&self) -> HashSet<u32> {
        self.runtimes
            .values()
            .flat_map(|runtime| [runtime.pid, runtime.launched.as_ref().and_then(|launched| launched.pid)])
            .flatten()
            .collect()
    }

    /// Sessions with a process, running or on their way out.
    pub fn active(&self) -> Vec<String> {
        self.runtimes.keys().cloned().collect()
    }

    fn is_current(&self, session_id: &str, instance: u64) -> bool {
        self.runtimes
            .get(session_id)
            .is_some_and(|runtime| runtime.instance == instance)
    }

    pub fn start(&mut self, session: &SessionRecord, requested: LaunchMode, cx: &mut Context<Self>) {
        let id = session.id.as_str();
        if let Some(previous) = self.runtimes.get_mut(id)
            && previous.pid.is_some()
            && !matches!(previous.status, SessionStatus::Ended | SessionStatus::Error)
        {
            // Started again before the old process was gone (e.g. reopened right after ending it):
            // it goes now, and whoever waits for it to end (its stop) is done; its exit is not
            // reported, being an older instance.
            previous.exited = true;
            let (pid, forget) = (
                previous.pid,
                previous.launched.as_ref().and_then(|launched| launched.pid),
            );
            let in_wsl = previous.wsl.is_some();
            self.kill_own(pid, forget, in_wsl, cx).detach();
        }
        self.instances += 1;
        let instance = self.instances;
        self.runtimes.insert(
            id.to_owned(),
            Runtime::new(instance, SessionStatus::Starting, requested),
        );

        if let Some(place) = parse_wsl_path(&session.cwd, Some(&self.deps.wsl.share)) {
            self.start_in_wsl(session, requested, instance, place, cx);
            cx.emit(SessionEvent::Changed(id.to_owned()));
            return;
        }
        if !Path::new(&session.cwd).exists() {
            return self.fail(id, SessionError::FolderMissing, cx);
        }
        let transcript =
            requested != LaunchMode::Resume || transcript_exists(&self.deps.claude_home, &session.claude_session_id);
        let Some(mode) = launch_mode(session.used == Some(true), requested, transcript) else {
            return self.fail(id, SessionError::TranscriptMissing, cx);
        };
        let Some(claude) = (self.deps.resolve_claude)() else {
            return self.fail(id, SessionError::ClaudeNotFound, cx);
        };
        let settings = write_hook_settings(
            &self.deps.settings_dir,
            self.deps.hooks_port,
            &self.deps.hooks_token,
            &session.id,
        );
        let settings = match settings {
            Ok(file) => file.to_string_lossy().into_owned(),
            Err(error) => {
                self.deps.log.error(
                    "hook settings could not be written",
                    Some(json!({ "error": error.to_string() })),
                );
                return self.fail(id, SessionError::SpawnFailed, cx);
            }
        };

        let mut started = json!({ "session": id, "claudeSession": session.claude_session_id, "mode": mode_name(mode) });
        if mode != requested {
            started["note"] = json!("no transcript yet, starting fresh under the same id");
        }
        started["cwd"] = json!(session.cwd);
        started["claude"] = json!(claude.source);
        self.deps.log.info("session start", Some(started));

        let args = claude_args(session, mode, Some(&settings));
        let mut launched = None;
        let (args, raw_args) = match &claude.via_cmd {
            Some(script) => {
                // The pty's process is cmd's then, not claude's: its registry entry is found by its
                // session id.
                launched = Some(Launched {
                    claude_session_id: session.claude_session_id.clone(),
                    pid: None,
                });
                if !self.via_cmd_logged {
                    self.via_cmd_logged = true;
                    self.deps.log.warn(
                        "claude is a launcher reframe does not know, it runs through cmd.exe",
                        Some(json!({ "claude": claude.source })),
                    );
                }
                (vec![cmd_command_line(script, &args)], true)
            }
            None => (claude.prefix_args.iter().cloned().chain(args).collect(), false),
        };
        let mut env = claude.env.clone();
        env.insert("REFRAME_SESSION".into(), session.id.clone());
        let launch = Launch {
            program: claude.file.clone(),
            args,
            raw_args,
            cwd: PathBuf::from(&session.cwd),
            env,
        };
        if let Some(runtime) = self.runtimes.get_mut(id) {
            runtime.mode = mode;
            runtime.launched = launched;
        }
        self.spawn(id, instance, &launch, cx);
    }

    /// Runs claude in the distribution of the session's WSL folder. Asking the distribution how
    /// (which starts it) takes a moment; the session shows as starting meanwhile, and is not
    /// started at all if it was ended by then.
    fn start_in_wsl(
        &mut self,
        session: &SessionRecord,
        requested: LaunchMode,
        instance: u64,
        place: WslPath,
        cx: &mut Context<Self>,
    ) {
        let wsl = self.deps.wsl.clone();
        let session = session.clone();
        cx.spawn(async move |this, cx| {
            // Ended, started again, or failed while the distribution was asked.
            let still = |this: &WeakEntity<Self>, cx: &mut AsyncApp| {
                this.update(cx, |this, _| {
                    this.runtimes.get(&session.id).is_some_and(|runtime| {
                        runtime.instance == instance
                            && runtime.stopping.is_none()
                            && runtime.status == SessionStatus::Starting
                    })
                })
                .unwrap_or(false)
            };
            let (info, folder) = {
                let (wsl, distro, cwd) = (wsl.clone(), place.distro.clone(), session.cwd.clone());
                cx.background_executor()
                    .spawn(async move {
                        let info = wsl.info(&distro);
                        let folder =
                            info.as_ref().is_some_and(|info| info.claude.is_some()) && Path::new(&cwd).is_dir();
                        (info, folder)
                    })
                    .await
            };
            if !still(&this, cx) {
                return;
            }
            let Some((info, claude)) = info.and_then(|info| info.claude.clone().map(|claude| (info, claude))) else {
                let _ = this.update(cx, |this, cx| this.fail(&session.id, SessionError::ClaudeNotFound, cx));
                return;
            };
            if !folder {
                let _ = this.update(cx, |this, cx| this.fail(&session.id, SessionError::FolderMissing, cx));
                return;
            }
            let home = PathBuf::from(wsl_windows_path(&place.distro, &info.claude_home, &wsl.share));
            // Over the share: without an answer in time, the transcript is taken to be there (a
            // resume without one says so).
            let transcript = requested != LaunchMode::Resume || {
                let (home, id) = (home.clone(), session.claude_session_id.clone());
                within(cx, Duration::from_millis(5000), true, move || {
                    transcript_exists(&home, &id)
                })
                .await
            };
            if !still(&this, cx) {
                return;
            }
            let Some(mode) = launch_mode(session.used == Some(true), requested, transcript) else {
                let _ = this.update(cx, |this, cx| {
                    this.fail(&session.id, SessionError::TranscriptMissing, cx)
                });
                return;
            };
            // The claude reframe last ran for it may still run in the distribution: reframe
            // crashed, and ending the wsl.exe in between does not reliably end what runs behind
            // it. Resumed next to it, the conversation would run twice, so it is ended first.
            if let Some(linux_pid) = session.linux_pid {
                let (wsl, home, distro) = (wsl.clone(), home.clone(), place.distro.clone());
                let (claude_id, id) = (session.claude_session_id.clone(), session.id.clone());
                let orphan = within(cx, SHARE_TIMEOUT, false, {
                    let (wsl, distro) = (wsl.clone(), distro.clone());
                    move || {
                        read_registry_entry(&home, linux_pid).is_some_and(|entry| entry.session_id == claude_id)
                            && wsl.is_claude(&distro, linux_pid)
                    }
                })
                .await;
                if orphan {
                    let _ = this.update(cx, |this, _| {
                        this.deps.log.warn(
                            "ending orphaned claude in wsl from a previous run",
                            Some(json!({ "session": id, "distro": distro, "linuxPid": linux_pid })),
                        );
                    });
                    cx.background_executor()
                        .spawn(async move { wsl.stop_claude(&distro, linux_pid) })
                        .await;
                }
                if !still(&this, cx) {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                let mut started = json!({
                    "session": session.id,
                    "claudeSession": session.claude_session_id,
                    "mode": mode_name(mode),
                });
                if mode != requested {
                    started["note"] = json!("no transcript yet, starting fresh under the same id");
                }
                started["cwd"] = json!(session.cwd);
                started["wsl"] = json!(place.distro);
                started["claude"] = json!(claude);
                this.deps.log.info("session start", Some(started));
                // Without a shell (so nothing the user's startup files print gets in), with the
                // PATH their login shell has.
                let mut args = wsl.command.prefix_args.clone();
                args.extend([
                    "-d".to_owned(),
                    place.distro.clone(),
                    "--cd".to_owned(),
                    place.path.clone(),
                    "--exec".to_owned(),
                    "env".to_owned(),
                    format!("PATH={}", info.path),
                    claude.clone(),
                ]);
                args.extend(claude_args(&session, mode, None));
                let launch = Launch {
                    program: wsl.command.file.to_string_lossy().into_owned(),
                    args,
                    raw_args: false,
                    cwd: std::env::var_os("USERPROFILE").map_or_else(std::env::temp_dir, PathBuf::from),
                    env: HashMap::new(),
                };
                if let Some(runtime) = this.runtimes.get_mut(&session.id) {
                    runtime.mode = mode;
                    runtime.started = Instant::now();
                    runtime.started_at_ms = now_ms();
                    runtime.wsl = Some(WslRuntime {
                        distro: place.distro.clone(),
                        home,
                        claude_session_id: session.claude_session_id.clone(),
                        linux_pid: None,
                        reading: false,
                    });
                }
                this.spawn(&session.id, instance, &launch, cx);
            });
        })
        .detach();
    }

    /// Starts the process of a session's start on a pseudo console.
    fn spawn(&mut self, id: &str, instance: u64, launch: &Launch, cx: &mut Context<Self>) {
        let (pty, events) = match Pty::spawn(launch) {
            Ok(spawned) => spawned,
            Err(error) => {
                self.deps.log.warn(
                    "session could not be spawned",
                    Some(json!({ "error": error.to_string() })),
                );
                return self.fail(id, SessionError::SpawnFailed, cx);
            }
        };
        let pid = pty.pid;
        let terminal = cx.new(|cx| Terminal::new(pty, events, cx));
        let subscription = cx.subscribe(&terminal, {
            let id = id.to_owned();
            move |this, _, event, cx| match event {
                TerminalEvent::Exited(code) => this.on_exit(&id, instance, *code, cx),
            }
        });
        if let Some(runtime) = self.runtimes.get_mut(id) {
            runtime.terminal = Some(terminal);
            runtime._terminal_events = Some(subscription);
            runtime.pid = pid;
            runtime.proc_start = pid.and_then(process_start);
        }
        if let Some(pid) = pid {
            self.deps
                .log
                .info("session spawned", Some(json!({ "session": id, "pid": pid })));
            cx.emit(SessionEvent::Identity(
                id.to_owned(),
                IdentityPatch {
                    pid: Some(pid),
                    ..IdentityPatch::default()
                },
            ));
        }
        cx.emit(SessionEvent::Changed(id.to_owned()));
    }

    /// Shows a session as not started, for a reason (its conversation runs elsewhere), without
    /// starting anything.
    #[allow(dead_code)] // until conversations running elsewhere are looked for
    pub fn refuse(&mut self, session_id: &str, error: SessionError, cx: &mut Context<Self>) {
        if self.is_running(session_id) {
            return;
        }
        self.instances += 1;
        let mut runtime = Runtime::new(self.instances, SessionStatus::Error, LaunchMode::Resume);
        runtime.error = Some(error);
        runtime.exited = true;
        self.runtimes.insert(session_id.to_owned(), runtime);
        self.deps.log.info(
            "session not started",
            Some(json!({ "session": session_id, "error": error.key() })),
        );
        cx.emit(SessionEvent::Changed(session_id.to_owned()));
    }

    /// Ends a session the way a user would (Ctrl+C), then kills the process tree if it does not
    /// exit. The task is done when the process is gone.
    pub fn stop(&mut self, session_id: &str, timeout: Duration, cx: &mut Context<Self>) -> Shared<Task<()>> {
        let Some(runtime) = self.runtimes.get(session_id) else {
            return Task::ready(()).shared();
        };
        // A second stop (e.g. removing a session that is still ending) joins the first one.
        if let Some(stopping) = &runtime.stopping {
            return stopping.clone();
        }
        let instance = runtime.instance;
        let id = session_id.to_owned();
        let task = cx
            .spawn(async move |this, cx| Self::stop_runtime(this, id, instance, timeout, cx).await)
            .shared();
        if let Some(runtime) = self.runtimes.get_mut(session_id) {
            runtime.stopping = Some(task.clone());
        }
        task
    }

    async fn stop_runtime(this: WeakEntity<Self>, id: String, instance: u64, timeout: Duration, cx: &mut AsyncApp) {
        let running = this.update(cx, |this, _| this.is_running(&id) && this.is_current(&id, instance));
        if running.unwrap_or(false) {
            let deadline = Instant::now() + timeout;
            for _ in 0..3 {
                if Instant::now() >= deadline {
                    break;
                }
                // Once the session was started again, Ctrl+C would hit the new process; start()
                // kills this one.
                let written = this.update(cx, |this, cx| {
                    let terminal = this.runtimes.get(&id).filter(|runtime| runtime.instance == instance)?;
                    terminal.terminal.as_ref()?.read(cx).pty().write(&b"\x03"[..]);
                    Some(())
                });
                if !matches!(written, Ok(Some(()))) {
                    break;
                }
                if Self::settle_exited(&this, &id, instance, Duration::from_millis(700), cx).await {
                    break;
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if !Self::settle_exited(&this, &id, instance, left, cx).await {
                let kill = this.update(cx, |this, cx| {
                    let runtime = this.runtimes.get(&id).filter(|runtime| runtime.instance == instance)?;
                    let pid = runtime.pid?;
                    this.deps.log.warn(
                        "session did not exit on Ctrl+C, killing its process tree",
                        Some(json!({ "session": id, "pid": pid })),
                    );
                    let forget = runtime.launched.as_ref().and_then(|launched| launched.pid);
                    // In WSL ending wsl.exe does not reliably end what runs behind it.
                    if let Some(wsl) = runtime.wsl.as_ref()
                        && let Some(linux_pid) = wsl.linux_pid
                    {
                        let (distros, distro) = (this.deps.wsl.clone(), wsl.distro.clone());
                        cx.background_executor()
                            .spawn(async move { distros.stop_claude(&distro, linux_pid) })
                            .detach();
                        return Some(this.kill_own(Some(pid), None, true, cx));
                    }
                    Some(this.kill_own(Some(pid), forget, false, cx))
                });
                if let Ok(Some(kill)) = kill {
                    kill.await;
                    Self::settle_exited(&this, &id, instance, Duration::from_millis(1000), cx).await;
                }
            }
        }
        // The pty goes with its terminal. Started again in the meantime: the new runtime stays.
        let _ = this.update(cx, |this, _| {
            if this.is_current(&id, instance) {
                this.runtimes.remove(&id);
            }
        });
    }

    /// Whether the process of that start of the session is gone within `limit`.
    async fn settle_exited(
        this: &WeakEntity<Self>,
        id: &str,
        instance: u64,
        limit: Duration,
        cx: &mut AsyncApp,
    ) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            let exited = this.update(cx, |this, _| match this.runtimes.get(id) {
                Some(runtime) if runtime.instance == instance => runtime.exited,
                _ => true,
            });
            if exited.unwrap_or(true) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            cx.background_executor().timer(Duration::from_millis(20)).await;
        }
    }

    pub fn stop_all(&mut self, timeout: Duration, cx: &mut Context<Self>) -> Task<()> {
        let stops: Vec<_> = self.active().iter().map(|id| self.stop(id, timeout, cx)).collect();
        cx.background_executor().spawn(async move {
            futures::future::join_all(stops).await;
        })
    }

    /// Kills processes of sessions that survived a crash of a previous run, so they are not
    /// resumed twice.
    pub fn cleanup_orphans(&self, sessions: &[SessionRecord], cx: &mut Context<Self>) -> Task<()> {
        let home = self.deps.claude_home.clone();
        let log = self.deps.log.clone();
        let candidates: Vec<(String, String, u32)> = sessions
            .iter()
            .filter_map(|session| Some((session.id.clone(), session.claude_session_id.clone(), session.pid?)))
            .collect();
        cx.background_executor().spawn(async move {
            for (id, claude_session_id, pid) in candidates {
                if !is_alive(pid) {
                    continue;
                }
                // Its entry alone is not enough: the process may be gone and its id another
                // program's now.
                let Some(entry) = read_registry_entry(&home, pid) else {
                    continue;
                };
                if entry.session_id == claude_session_id && is_running_entry(&entry, &is_alive, &process_start) {
                    log.warn(
                        "killing orphaned claude from a previous run",
                        Some(json!({ "session": id, "pid": pid })),
                    );
                    kill_tree(pid);
                    forget_registry_entry(&home, pid, &is_alive);
                }
            }
        })
    }

    /// Kills a session's process tree, and removes the registry entry the claude could not remove
    /// itself. Through cmd.exe the entry is under claude's own process id (`forget`).
    /// In WSL (`in_wsl`) the entry is the Linux process's, in the distribution, and left alone.
    fn kill_own(&self, pid: Option<u32>, forget: Option<u32>, in_wsl: bool, cx: &mut Context<Self>) -> Task<()> {
        let home = self.deps.claude_home.clone();
        cx.background_executor().spawn(async move {
            let Some(pid) = pid else { return };
            kill_tree(pid);
            if !in_wsl {
                forget_registry_entry(&home, forget.unwrap_or(pid), &is_alive);
            }
        })
    }

    fn on_exit(&mut self, session_id: &str, instance: u64, exit_code: i32, cx: &mut Context<Self>) {
        // A process that is still going away while its session was started again: not this
        // runtime's business.
        let Some(runtime) = self
            .runtimes
            .get_mut(session_id)
            .filter(|runtime| runtime.instance == instance)
        else {
            return;
        };
        if matches!(runtime.status, SessionStatus::Ended | SessionStatus::Error) {
            runtime.exited = true;
            return;
        }
        let by_reframe = runtime.stopping.is_some();
        let ran_for = runtime.started.elapsed().as_secs_f64() * 1000.0;
        let (status, error) = exit_outcome(by_reframe, ran_for, exit_code, runtime.mode);
        runtime.status = status;
        runtime.error = error;
        runtime.exited = true;
        let mut data = json!({ "session": session_id, "exitCode": exit_code, "status": status_name(status) });
        if let Some(error) = error {
            data["error"] = json!(error.key());
        }
        data["byReframe"] = json!(by_reframe);
        if error.is_some() {
            self.deps.log.warn("session exited", Some(data));
        } else {
            self.deps.log.info("session exited", Some(data));
        }
        cx.emit(SessionEvent::Changed(session_id.to_owned()));
        if !by_reframe {
            cx.emit(SessionEvent::Ended(session_id.to_owned()));
        }
    }

    fn fail(&mut self, session_id: &str, error: SessionError, cx: &mut Context<Self>) {
        let Some(runtime) = self.runtimes.get_mut(session_id) else {
            return;
        };
        runtime.status = SessionStatus::Error;
        runtime.error = Some(error);
        runtime.exited = true;
        self.deps.log.warn(
            "session could not start",
            Some(json!({ "session": session_id, "error": error.key() })),
        );
        cx.emit(SessionEvent::Changed(session_id.to_owned()));
    }

    fn on_hook(&mut self, message: HookMessage, cx: &mut Context<Self>) {
        let id = message.session_id.as_str();
        let event = message.event.as_str();
        let known = self.runtimes.contains_key(id);
        let mut data = json!({ "session": id, "event": event });
        if let Some(kind) = message
            .payload
            .notification_type
            .as_deref()
            .filter(|kind| !kind.is_empty())
        {
            data["type"] = json!(kind);
        }
        if !known {
            data["ignored"] = json!("unknown session");
        }
        self.deps.log.info("hook", Some(data));
        if !known || !self.is_running(id) {
            return;
        }
        let effects = hook_effects(
            event,
            message.payload.session_id.as_deref(),
            message.payload.cwd.as_deref(),
            message.payload.notification_type.as_deref(),
        );
        if !effects.identity.is_empty() {
            cx.emit(SessionEvent::Identity(id.to_owned(), effects.identity));
        }
        if let Some(status) = effects.status {
            self.set_status(id, status, cx);
        }
        if let Some(kind) = effects.notify {
            cx.emit(SessionEvent::Notify(id.to_owned(), kind));
        }
    }

    fn poll_registry(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.runtimes.keys().cloned().collect();
        for id in ids {
            if !self.is_running(&id) {
                continue;
            }
            let home = &self.deps.claude_home;
            let Some(runtime) = self.runtimes.get_mut(&id) else {
                continue;
            };
            let Some(pid) = runtime.pid else { continue };
            if runtime.wsl.is_some() {
                self.poll_wsl_registry(&id, cx);
                continue;
            }
            let entry = match &mut runtime.launched {
                // Found by its session id once, then by its process id.
                Some(launched) => {
                    let known = launched.pid.and_then(|pid| read_registry_entry(home, pid));
                    let found = known.or_else(|| {
                        let running = |entry: &RegistryEntry| is_running_entry(entry, &is_alive, &process_start);
                        find_registry_entry(home, &launched.claude_session_id, Some(&running))
                    });
                    launched.pid = found.as_ref().map(|entry| entry.pid);
                    found
                }
                None => read_registry_entry(home, pid),
            };
            // Nothing (yet).
            let Some(entry) = entry else { continue };
            let place = if runtime.launched.is_some() {
                RegistryPlace::ThroughCmd
            } else {
                RegistryPlace::Direct
            };
            if place == RegistryPlace::Direct
                && is_stale_entry(&entry, runtime.proc_start.as_deref(), runtime.started_at_ms)
            {
                if !runtime.stale_seen {
                    runtime.stale_seen = true;
                    self.deps.log.info(
                        "registry entry left behind by an earlier process, ignored",
                        Some(json!({ "session": id })),
                    );
                }
                continue;
            }
            let effects = follow_registry(&entry, &mut runtime.registry, runtime.status, place);
            cx.emit(SessionEvent::Identity(id.clone(), effects.identity));
            if effects.name_changed {
                cx.emit(SessionEvent::Changed(id.clone()));
            }
            if let Some(status) = effects.status {
                self.set_status(&id, status, cx);
            }
            if let Some(kind) = effects.notify {
                cx.emit(SessionEvent::Notify(id.clone(), kind));
            }
        }
    }

    /// A WSL session's registry entry, read over the share. It is named after the Linux process
    /// id, which only the entry itself tells, so it is found by the session id first (and again
    /// should that file go away). A read still waiting for the share is not started a second
    /// time.
    fn poll_wsl_registry(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(runtime) = self.runtimes.get_mut(id) else {
            return;
        };
        let instance = runtime.instance;
        let Some(wsl) = runtime.wsl.as_mut().filter(|wsl| !wsl.reading) else {
            return;
        };
        wsl.reading = true;
        let (home, claude_id, known, distro) = (
            wsl.home.clone(),
            wsl.claude_session_id.clone(),
            wsl.linux_pid,
            wsl.distro.clone(),
        );
        let distros = self.deps.wsl.clone();
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            let read = {
                let distro = distro.clone();
                move || {
                    known.and_then(|pid| read_registry_entry(&home, pid)).or_else(|| {
                        let claude = |entry: &RegistryEntry| distros.is_claude(&distro, entry.pid);
                        find_registry_entry(&home, &claude_id, Some(&claude))
                    })
                }
            };
            let entry = within(cx, SHARE_TIMEOUT, None, move || Some(read())).await;
            let _ = this.update(cx, |this, cx| {
                let share = this.deps.wsl.share.clone();
                let answered = entry.is_some();
                let entry = entry.flatten();
                let slow = !answered
                    && this
                        .slow_share_logged
                        .is_none_or(|at| at.elapsed() > Duration::from_secs(60));
                if slow {
                    this.slow_share_logged = Some(Instant::now());
                    this.deps.log.warn(
                        "a wsl distribution did not answer in time",
                        Some(json!({ "distro": distro })),
                    );
                }
                let Some(runtime) = this
                    .runtimes
                    .get_mut(&id)
                    .filter(|runtime| runtime.instance == instance)
                else {
                    return;
                };
                let Some(wsl) = runtime.wsl.as_mut() else { return };
                wsl.reading = false;
                let Some(entry) = entry else {
                    if answered {
                        wsl.linux_pid = None;
                    }
                    return;
                };
                wsl.linux_pid = Some(entry.pid);
                if !is_running(runtime.status) {
                    return;
                }
                let place = RegistryPlace::Wsl {
                    distro: &distro,
                    share: &share,
                };
                let effects = follow_registry(&entry, &mut runtime.registry, runtime.status, place);
                cx.emit(SessionEvent::Identity(id.clone(), effects.identity));
                if effects.name_changed {
                    cx.emit(SessionEvent::Changed(id.clone()));
                }
                if let Some(status) = effects.status {
                    this.set_status(&id, status, cx);
                }
                if let Some(kind) = effects.notify {
                    cx.emit(SessionEvent::Notify(id.clone(), kind));
                }
            });
        })
        .detach();
    }

    fn set_status(&mut self, session_id: &str, status: SessionStatus, cx: &mut Context<Self>) {
        let Some(runtime) = self.runtimes.get_mut(session_id) else {
            return;
        };
        if runtime.status == status {
            return;
        }
        let used = counts_as_use(runtime.status, status);
        runtime.status = status;
        if used {
            cx.emit(SessionEvent::Used(session_id.to_owned()));
        }
        cx.emit(SessionEvent::Changed(session_id.to_owned()));
    }
}

fn mode_name(mode: LaunchMode) -> &'static str {
    match mode {
        LaunchMode::New => "new",
        LaunchMode::Resume => "resume",
    }
}

pub fn status_name(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Starting => "starting",
        SessionStatus::Idle => "idle",
        SessionStatus::Busy => "busy",
        SessionStatus::Waiting => "waiting",
        SessionStatus::Ended => "ended",
        SessionStatus::Error => "error",
    }
}
