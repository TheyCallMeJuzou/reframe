//! Wires state, sessions and hooks together. The controller owns the state: windows send intents,
//! it applies them, saves, and tells the windows what changed.
//!
//! Ported from the Electron version's `src/main/app.ts`, as far as sessions go. What follows with
//! later milestones is named where it would be: asking before a conversation that runs elsewhere
//! is taken over, sessions in WSL, the tray, the session list, the system going down.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::FutureExt;
use futures::channel::mpsc::unbounded;
use gpui_kit::*;
use reframe_core::actions::{
    LayoutAction, add_tile, apply_layout_action, drop_session, is_window_empty, window_has_session, windows_to_restore,
};
use reframe_core::claude::args::LaunchMode;
use reframe_core::claude::registry::claude_home;
use reframe_core::claude::resolve::{self, ClaudeBinary, NodeRunner, Platform, ResolveOptions, resolve_claude};
use reframe_core::cleanup::{clean_up, drops_dir};
use reframe_core::hooks::HookServer;
use reframe_core::i18n::{Locale, locale_of, resolve_locale};
use reframe_core::layout::DropTarget;
use reframe_core::log::Logger;
use reframe_core::paths::home_relative;
use reframe_core::session_logic::{IdentityPatch, SessionError, SessionStatus};
use reframe_core::shortcuts::{TextSizeChange, font_size_after};
use reframe_core::state::{
    AppState, LaunchOptions, Lifecycle, PermissionMode, Preferences, SessionRecord, WindowState,
};
use reframe_core::store::{LoadSource, SAVE_DELAY, StateStore};
use reframe_core::wsl::parse_wsl_path;
use serde_json::json;
use uuid::Uuid;

use crate::sessions::{SessionEvent, SessionManager, SessionManagerDeps};
use crate::terminal_view::TerminalFontSize;

const MAX_RECENT_FOLDERS: usize = 8;
/// How long a session gets to end on Ctrl+C before its process tree is killed.
const STOP_MS: Duration = Duration::from_millis(5000);
/// How long quitting waits for the sessions to end (each gets Ctrl+C, then its process tree is
/// killed).
const SHUTDOWN_MS: Duration = Duration::from_millis(15_000);

/// Where this copy of reframe keeps its things, and what it was started with.
#[derive(Clone)]
pub struct Config {
    /// The profile: `state.json`, `logs`, `hooks`.
    pub user_data: PathBuf,
    pub env: HashMap<String, String>,
    pub home: PathBuf,
    /// Driven by a test (`REFRAME_E2E`): no dialogs, windows stay hidden.
    pub e2e: bool,
    /// The language Windows shows itself in (`REFRAME_LOCALE` in its place, for tests), as
    /// "de-AT".
    pub system_locale: String,
}

/// The name of the user's language in Windows, as "de-AT".
fn windows_locale() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut name = [0u16; 85];
    // SAFETY: the buffer is as long as the length says.
    let length = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    // The length counts the terminating zero.
    String::from_utf16_lossy(&name[..usize::try_from(length - 1).unwrap_or(0)])
}

impl Config {
    /// From this process's environment. Until the native app takes over from the Electron one it
    /// keeps a profile of its own (`reframe-native`), so the two never write the same state file.
    pub fn from_environment() -> Config {
        let env: HashMap<String, String> = std::env::vars().collect();
        let set = |name: &str| env.get(name).filter(|value| !value.is_empty()).cloned();
        let home = PathBuf::from(set("USERPROFILE").unwrap_or_default());
        let user_data = match set("REFRAME_USER_DATA") {
            Some(dir) => PathBuf::from(dir),
            None => set("APPDATA")
                .map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
                .join("reframe-native"),
        };
        let e2e = set("REFRAME_E2E").is_some();
        let system_locale = set("REFRAME_LOCALE").unwrap_or_else(windows_locale);
        Config {
            user_data,
            env,
            home,
            e2e,
            system_locale,
        }
    }
}

/// What a new session is to be.
#[derive(Clone, Debug, Default)]
pub struct NewSessionRequest {
    pub cwd: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    pub worktree: bool,
    pub worktree_name: Option<String>,
    /// Where its tile goes; at the end without one.
    pub target: Option<DropTarget>,
}

// Whose it is: read by the window that follows with the next milestone.
#[allow(dead_code)]
pub enum AppEvent {
    /// Something about a session changed that its tile shows.
    Session(String),
    /// A window's layout changed.
    Window(String),
    /// A window is gone.
    WindowClosed(String),
    /// A preference changed that windows show (the language, the text size, …).
    Preferences,
}

/// Why reframe does not start.
#[derive(Debug)]
pub enum StartError {
    /// The saved state comes from a newer reframe; it is left alone.
    NewerState(PathBuf),
    Hooks(std::io::Error),
}

pub struct Controller {
    config: Config,
    store: StateStore,
    state: AppState,
    log: Logger,
    hooks: HookServer,
    sessions: Entity<SessionManager>,
    /// The claude chosen in Settings, where the session manager's resolver reads it.
    claude_bin: Rc<RefCell<Option<String>>>,
    shutting_down: bool,
    /// The write that follows frequent changes after a moment.
    save_soon: Option<Task<()>>,
    _session_events: Subscription,
}

impl EventEmitter<AppEvent> for Controller {}

fn now_ms() -> f64 {
    // Whole milliseconds, as JavaScript's Date.now().
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_millis() as f64)
}

fn find_claude(config: &Config, chosen: Option<&str>) -> Option<ClaudeBinary> {
    let set = |name: &str| config.env.get(name).filter(|value| !value.is_empty());
    let home = config.home.to_string_lossy();
    resolve_claude(&ResolveOptions {
        env: &config.env,
        platform: Platform::Windows,
        override_path: chosen
            .filter(|chosen| !chosen.is_empty())
            .or(set("REFRAME_CLAUDE_BIN").map(String::as_str)),
        // Tests: a .js stand-in for claude, run by a real node.
        node_runner: set("REFRAME_NODE").map(|node| NodeRunner {
            file: node.clone(),
            env: HashMap::new(),
        }),
        home: Some(&home),
        is_file: &resolve::is_file,
        read_file: &resolve::read_file,
    })
}

/// Whether two paths name the same folder, as Windows compares them.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |path: &str| path.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

impl Controller {
    /// Loads the state and gets everything ready that sessions need. Nothing is started yet: see
    /// [`Controller::cleanup_orphans`] and [`Controller::resume_sessions`].
    pub fn new(config: Config, cx: &mut Context<Self>) -> Result<Self, StartError> {
        let log = Logger::new(config.user_data.join("logs"));
        let mut store = StateStore::new(config.user_data.join("state.json"));
        let loaded = match store.load() {
            Ok(loaded) => loaded,
            Err(error) => {
                // The unusable file could not be set aside; it is written over with the next save.
                log.error(
                    "state file could not be set aside",
                    Some(json!({ "error": error.to_string() })),
                );
                reframe_core::store::LoadResult {
                    state: AppState::default(),
                    source: LoadSource::Default,
                    rejected: None,
                }
            }
        };
        if loaded.source == LoadSource::Newer {
            log.error(
                "state file is from a newer reframe, leaving it alone and quitting",
                Some(json!({ "file": store.file() })),
            );
            return Err(StartError::NewerState(store.file().to_owned()));
        }
        if let Some(rejected) = &loaded.rejected {
            log.error(
                "state file could not be used, set aside",
                Some(json!({ "file": rejected.file, "reason": rejected.reason })),
            );
        }
        let mut state = loaded.state;
        if state.windows.is_empty() {
            state.windows.push(WindowState::empty(Uuid::new_v4().to_string()));
        }
        // Empty windows are not brought back (one stays when all are empty), and the first window
        // with something in it is the first window from now on.
        let windows = windows_to_restore(&state.windows);
        let dropped = state.windows.len() - windows.len();
        if dropped > 0 {
            state.windows = windows;
            let _ = store.save_now(&state);
        }

        let claude_home = claude_home(&config.env, &config.home);
        cx.set_global(TerminalFontSize(f32::from(state.preferences.font_size)));
        let claude_bin = Rc::new(RefCell::new(state.preferences.claude_bin.clone()));
        let found = find_claude(&config, claude_bin.borrow().as_deref());
        let mut starting = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "platform": "win32",
            "build": "native",
            "state": match loaded.source {
                LoadSource::File => "file",
                LoadSource::Backup => "backup",
                LoadSource::Default | LoadSource::Newer => "default",
            },
            "windows": state.windows.len(),
        });
        if dropped > 0 {
            starting["emptyWindowsDropped"] = json!(dropped);
        }
        starting["sessions"] = json!(state.sessions.len());
        starting["claude"] = json!(found.map_or_else(|| "not found".to_owned(), |claude| claude.source));
        starting["claudeHome"] = json!(claude_home);
        log.info("reframe starting", Some(starting));

        let (hook_sender, hook_receiver) = unbounded();
        let hooks = HookServer::start(move |message| {
            let _ = hook_sender.unbounded_send(message);
        })
        .map_err(StartError::Hooks)?;

        let deps = SessionManagerDeps {
            claude_home,
            settings_dir: config.user_data.join("hooks"),
            hooks_port: hooks.port(),
            hooks_token: hooks.token().to_owned(),
            resolve_claude: Box::new({
                let (config, chosen) = (config.clone(), claude_bin.clone());
                move || find_claude(&config, chosen.borrow().as_deref())
            }),
            log: log.clone(),
        };
        let sessions = cx.new(|cx| SessionManager::new(deps, hook_receiver, cx));
        let session_events = cx.subscribe(&sessions, |this, _, event, cx| match event {
            SessionEvent::Changed(id) => cx.emit(AppEvent::Session(id.clone())),
            SessionEvent::Identity(id, patch) => this.update_identity(id, patch, cx),
            // Notifications follow with the tray (milestone 5).
            SessionEvent::Notify(..) => {}
            SessionEvent::Used(id) => this.session_used(id, cx),
            SessionEvent::Ended(id) => this.session_ended(id, cx),
        });

        Ok(Self {
            config,
            store,
            state,
            log,
            hooks,
            sessions,
            claude_bin,
            shutting_down: false,
            save_soon: None,
            _session_events: session_events,
        })
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn sessions(&self) -> &Entity<SessionManager> {
        &self.sessions
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The language reframe speaks: the one chosen in Settings, else the system's.
    pub fn locale(&self) -> Locale {
        locale_of(
            self.state.preferences.language,
            resolve_locale(&self.config.system_locale),
        )
    }

    /// Where the claude is that sessions run, if there is one.
    pub fn claude_source(&self) -> Option<String> {
        find_claude(&self.config, self.claude_bin.borrow().as_deref()).map(|claude| claude.source)
    }

    /// The folders sessions were started in last, without those that are gone.
    pub fn recent_folders(&self) -> Vec<String> {
        let there = |folder: &&String| parse_wsl_path(folder, None).is_some() || Path::new(folder.as_str()).is_dir();
        self.state.recent_folders.iter().filter(there).cloned().collect()
    }

    /// Changes preferences (Settings, the text size keys); every window shows them at once.
    pub fn update_preferences(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Preferences)) {
        let was = self.state.preferences.clone();
        change(&mut self.state.preferences);
        let now = &self.state.preferences;
        if *now == was {
            return;
        }
        if now.claude_bin != was.claude_bin {
            let claude = now
                .claude_bin
                .clone()
                .unwrap_or_else(|| "the one on the PATH".to_owned());
            self.log
                .info("claude to run changed", Some(json!({ "claude": claude })));
            *self.claude_bin.borrow_mut() = now.claude_bin.clone();
        }
        if now.language != was.language {
            let language = serde_json::to_value(now.language).unwrap_or_default();
            self.log.info(
                "language changed",
                Some(json!({ "language": language, "locale": self.locale().as_str() })),
            );
        }
        if now.font_size != was.font_size {
            cx.set_global(TerminalFontSize(f32::from(now.font_size)));
            cx.refresh_windows();
        }
        self.save(cx);
        cx.emit(AppEvent::Preferences);
    }

    /// The terminals' text a step bigger or smaller, or back to normal.
    pub fn change_text_size(&mut self, change: TextSizeChange, cx: &mut Context<Self>) {
        self.update_preferences(cx, |preferences| {
            preferences.font_size = font_size_after(preferences.font_size, change);
        });
    }

    /// A name the user gives a session (in reframe only); an empty one goes back to the
    /// automatic name.
    pub fn rename_session(&mut self, session_id: &str, name: &str, cx: &mut Context<Self>) {
        let trimmed: String = name.trim().chars().take(100).collect();
        let automatic = trimmed.is_empty();
        self.update_session(session_id, true, cx, |record| {
            // Claude Code keeps the name it was started with; renaming is reframe's own (shown
            // instead of it).
            if record.claude_name.is_none() {
                record.claude_name = Some(record.name.clone());
            }
            record.name = (!automatic).then_some(trimmed);
        });
        self.log.info(
            "session renamed",
            Some(json!({ "session": session_id, "automatic": automatic })),
        );
    }

    fn find_window(&self, window_id: &str) -> Option<&WindowState> {
        self.state.windows.iter().find(|window| window.id == window_id)
    }

    /// The window a session's tile (or collapsed tile) is in.
    pub fn window_of(&self, session_id: &str) -> Option<&WindowState> {
        self.state
            .windows
            .iter()
            .find(|window| window_has_session(window, session_id))
    }

    // ---------- start ----------

    /// Ends processes of sessions that survived a crash of a previous run, so they are not resumed
    /// twice. Done before anything is resumed.
    pub fn cleanup_orphans(&self, cx: &mut Context<Self>) -> Task<()> {
        let records: Vec<SessionRecord> = self.state.sessions.values().cloned().collect();
        self.sessions
            .update(cx, |sessions, cx| sessions.cleanup_orphans(&records, cx))
    }

    /// Starts the sessions that were open, each resuming its conversation.
    pub fn resume_sessions(&mut self, cx: &mut Context<Self>) {
        let open: Vec<String> = self
            .state
            .sessions
            .values()
            .filter(|record| record.lifecycle == Lifecycle::Open)
            .map(|record| record.id.clone())
            .collect();
        for id in open {
            if self.window_of(&id).is_some() {
                self.start_session(&id, LaunchMode::Resume, cx);
            } else {
                self.update_session(&id, false, cx, |record| record.lifecycle = Lifecycle::Ended);
            }
        }
        self.save(cx);
        self.clean_up_files(cx);
    }

    /// Old dropped files and the hook settings of sessions that are gone.
    fn clean_up_files(&self, cx: &mut Context<Self>) {
        let hooks = self.config.user_data.join("hooks");
        let ids: HashSet<String> = self.state.sessions.keys().cloned().collect();
        let log = self.log.clone();
        cx.background_executor()
            .spawn(async move {
                let removed = clean_up(&drops_dir(), &hooks, &ids);
                if removed.drops > 0 || removed.hooks > 0 {
                    log.info(
                        "old dropped files and hook settings removed",
                        Some(json!({ "drops": removed.drops, "hooks": removed.hooks })),
                    );
                }
            })
            .detach();
    }

    // ---------- saving ----------

    fn save(&mut self, _: &mut Context<Self>) {
        self.save_soon = None;
        if let Err(error) = self.store.save_now(&self.state) {
            self.log
                .error("state could not be saved", Some(json!({ "error": error.to_string() })));
        }
    }

    /// Changes that come often (a session used, a window moved): written in a moment, once.
    fn save_later(&mut self, cx: &mut Context<Self>) {
        if !self.store.mark_dirty() && self.save_soon.is_some() {
            return;
        }
        self.save_soon = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, _| {
                if let Err(error) = this.store.flush(&this.state) {
                    this.log
                        .error("state could not be saved", Some(json!({ "error": error.to_string() })));
                }
            });
        }));
    }

    fn mutate_window(
        &mut self,
        window_id: &str,
        cx: &mut Context<Self>,
        change: impl FnOnce(&WindowState) -> WindowState,
    ) {
        if let Some(window) = self.state.windows.iter_mut().find(|window| window.id == window_id) {
            *window = change(window);
        }
        self.save(cx);
        cx.emit(AppEvent::Window(window_id.to_owned()));
    }

    /// Secondary windows go away once nothing is left in them; the first window always stays.
    fn close_if_empty(&mut self, window_id: &str, cx: &mut Context<Self>) {
        let first = self.state.windows.first().is_some_and(|window| window.id == window_id);
        if first || !self.find_window(window_id).is_some_and(is_window_empty) {
            return;
        }
        self.state.windows.retain(|window| window.id != window_id);
        self.save(cx);
        self.log
            .info("empty window closed", Some(json!({ "window": window_id })));
        cx.emit(AppEvent::WindowClosed(window_id.to_owned()));
    }

    // ---------- sessions ----------

    fn update_session(
        &mut self,
        session_id: &str,
        persist: bool,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut SessionRecord),
    ) {
        let Some(record) = self.state.sessions.get_mut(session_id) else {
            return;
        };
        change(record);
        if persist {
            self.save(cx);
        }
        cx.emit(AppEvent::Session(session_id.to_owned()));
    }

    fn update_identity(&mut self, session_id: &str, patch: &IdentityPatch, cx: &mut Context<Self>) {
        let Some(record) = self.state.sessions.get(session_id) else {
            return;
        };
        let mut next = patch.clone();
        // A new claude session id (after /clear) starts unused unless the same update says
        // otherwise.
        if patch
            .claude_session_id
            .as_ref()
            .is_some_and(|id| *id != record.claude_session_id)
            && patch.used.is_none()
        {
            next.used = Some(false);
        }
        // A worktree session whose folder moved runs in the worktree Claude Code made for it.
        let into_worktree = record.launch.worktree == Some(true)
            && record.launch.worktree_created != Some(true)
            && patch
                .cwd
                .as_ref()
                .is_some_and(|cwd| !cwd.is_empty() && !same_path(cwd, &record.cwd));
        let changed = into_worktree
            || next
                .claude_session_id
                .as_ref()
                .is_some_and(|id| *id != record.claude_session_id)
            || next.cwd.as_ref().is_some_and(|cwd| *cwd != record.cwd)
            || next.pid.is_some_and(|pid| Some(pid) != record.pid)
            || next.linux_pid.is_some_and(|pid| Some(pid) != record.linux_pid)
            || next.used.is_some_and(|used| Some(used) != record.used);
        if !changed {
            return;
        }
        self.update_session(session_id, true, cx, |record| {
            if let Some(id) = next.claude_session_id {
                record.claude_session_id = id;
            }
            if let Some(cwd) = next.cwd {
                record.cwd = cwd;
            }
            if next.pid.is_some() {
                record.pid = next.pid;
            }
            if next.linux_pid.is_some() {
                record.linux_pid = next.linux_pid;
            }
            if next.used.is_some() {
                record.used = next.used;
            }
            if into_worktree {
                record.launch.worktree_created = Some(true);
            }
        });
    }

    /// A session was used (see `counts_as_use`): the list shows it as just used, and puts it first.
    fn session_used(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(record) = self.state.sessions.get_mut(session_id) else {
            return;
        };
        record.last_active_at = now_ms();
        // Every prompt and answer: written with the next save, or in a moment.
        self.save_later(cx);
    }

    /// A session ended by itself (not by reframe): it stays in its tile as ended.
    fn session_ended(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.update_session(session_id, true, cx, |record| record.lifecycle = Lifecycle::Ended);
    }

    /// Starts a session's process. A conversation that runs outside reframe already is not looked
    /// for yet (that comes with the session list, which knows what runs elsewhere).
    fn start_session(&mut self, session_id: &str, mode: LaunchMode, cx: &mut Context<Self>) {
        let Some(record) = self.state.sessions.get(session_id).cloned() else {
            return;
        };
        if self.window_of(session_id).is_none() || self.sessions.read(cx).keeps_running(session_id) {
            return;
        }
        self.sessions
            .update(cx, |sessions, cx| sessions.start(&record, mode, cx));
    }

    /// A new session with a tile in that window; its id, or why there is none.
    pub fn create_session(
        &mut self,
        window_id: &str,
        request: NewSessionRequest,
        cx: &mut Context<Self>,
    ) -> Result<String, SessionError> {
        let trimmed = |text: Option<String>| text.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty());
        let cwd = request.cwd.trim().to_owned();
        if !Path::new(&cwd).is_dir() {
            return Err(SessionError::FolderMissing);
        }
        // Sessions in WSL folders follow with the WSL milestone.
        if parse_wsl_path(&cwd, None).is_some()
            || find_claude(&self.config, self.claude_bin.borrow().as_deref()).is_none()
        {
            return Err(SessionError::ClaudeNotFound);
        }
        if self.find_window(window_id).is_none() {
            return Err(SessionError::SpawnFailed);
        }
        let now = now_ms();
        let record = SessionRecord {
            id: Uuid::new_v4().to_string(),
            claude_session_id: Uuid::new_v4().to_string(),
            cwd: cwd.clone(),
            name: trimmed(request.name),
            claude_name: None,
            launch: LaunchOptions {
                model: trimmed(request.model),
                permission_mode: request.permission_mode,
                worktree: request.worktree.then_some(true),
                worktree_name: trimmed(request.worktree_name).filter(|_| request.worktree),
                worktree_created: None,
            },
            lifecycle: Lifecycle::Open,
            created_at: now,
            last_active_at: now,
            pid: None,
            linux_pid: None,
            used: None,
        };
        let id = record.id.clone();
        self.state.sessions.insert(id.clone(), record.clone());
        let key = cwd.to_lowercase();
        self.state.recent_folders.retain(|folder| folder.to_lowercase() != key);
        self.state.recent_folders.insert(0, cwd);
        self.state.recent_folders.truncate(MAX_RECENT_FOLDERS);
        self.mutate_window(window_id, cx, |window| add_tile(window, &id, request.target.as_ref()));
        self.sessions
            .update(cx, |sessions, cx| sessions.start(&record, LaunchMode::New, cx));
        Ok(id)
    }

    /// Starts an ended session again: its conversation resumed, or (`fresh`) a new one in its
    /// place.
    pub fn restart_session(&mut self, session_id: &str, fresh: bool, cx: &mut Context<Self>) {
        if self.sessions.read(cx).is_running(session_id) {
            return;
        }
        let Some(record) = self.state.sessions.get_mut(session_id) else {
            return;
        };
        if fresh {
            // A fresh start has had no prompt yet, whatever the session before it had; it is a new
            // conversation, as new as a new session. Resumed, it was last used when it was last
            // used.
            record.claude_session_id = Uuid::new_v4().to_string();
            record.used = Some(false);
            record.last_active_at = now_ms();
        }
        record.lifecycle = Lifecycle::Open;
        self.save(cx);
        self.start_session(session_id, if fresh { LaunchMode::New } else { LaunchMode::Resume }, cx);
        cx.emit(AppEvent::Session(session_id.to_owned()));
    }

    /// Ends a session and takes its tile away. It stays in the list as ended.
    pub fn end_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if !self.state.sessions.contains_key(session_id) {
            return;
        }
        let window_id = self.window_of(session_id).map(|window| window.id.clone());
        self.log
            .info("session ended by the user", Some(json!({ "session": session_id })));
        // It ends in the background; the session manager holds on to it.
        drop(
            self.sessions
                .update(cx, |sessions, cx| sessions.stop(session_id, STOP_MS, cx)),
        );
        if let Some(record) = self.state.sessions.get_mut(session_id) {
            record.lifecycle = Lifecycle::Ended;
        }
        match window_id {
            Some(window_id) => {
                self.mutate_window(&window_id, cx, |window| drop_session(window, session_id));
                self.close_if_empty(&window_id, cx);
            }
            None => self.save(cx),
        }
    }

    /// Forgets an ended session that is in no window; Claude Code's transcript is left alone.
    pub fn remove_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if !self.state.sessions.contains_key(session_id) || self.window_of(session_id).is_some() {
            return;
        }
        // Still ending right after its tile was closed: let it finish in the background.
        // It ends in the background; the session manager holds on to it.
        drop(
            self.sessions
                .update(cx, |sessions, cx| sessions.stop(session_id, STOP_MS, cx)),
        );
        self.state.sessions.shift_remove(session_id);
        self.save(cx);
        let _ = std::fs::remove_file(self.config.user_data.join("hooks").join(format!("{session_id}.json")));
        self.log
            .info("session removed from the list", Some(json!({ "session": session_id })));
    }

    // ---------- quitting ----------

    /// Ends all sessions the way a user would, saves, and quits. Sessions stay open and resume
    /// next time.
    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.log.info("reframe quitting, ending sessions", None);
        self.save(cx);
        let stopping = self.sessions.update(cx, |sessions, cx| sessions.stop_all(STOP_MS, cx));
        cx.spawn(async move |this, cx| {
            // Quitting must not hang on a session that does not go: what is left is ended as an
            // orphan on the next start.
            let ended = futures::select_biased! {
                () = stopping.fuse() => true,
                () = cx.background_executor().timer(SHUTDOWN_MS).fuse() => false,
            };
            let _ = this.update(cx, |this, cx| {
                if !ended {
                    let left = this.sessions.read(cx).active();
                    this.log.warn(
                        "sessions did not end in time, quitting anyway",
                        Some(json!({ "sessions": left })),
                    );
                }
                this.hooks.stop();
                this.save(cx);
                this.log.info("reframe stopped", None);
            });
            cx.update(|cx| cx.quit());
        })
        .detach();
    }
}

/// What a tile shows of its session.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionView {
    pub id: String,
    /// The user's name for it, else Claude's name for the session, else the folder.
    pub label: String,
    pub cwd: String,
    /// `cwd` as shown: under the home folder from "~".
    pub cwd_label: String,
    pub status: SessionStatus,
    pub error: Option<SessionError>,
}

impl Controller {
    pub fn session_view(&self, session_id: &str, cx: &App) -> Option<SessionView> {
        let record = self.state.sessions.get(session_id)?;
        let sessions = self.sessions.read(cx);
        let ended = if record.lifecycle == Lifecycle::Ended {
            SessionStatus::Ended
        } else {
            SessionStatus::Starting
        };
        let folder = record
            .cwd
            .trim_end_matches(['\\', '/'])
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default();
        let named = |name: Option<&str>| name.filter(|name| !name.is_empty()).map(str::to_owned);
        let label = named(record.name.as_deref())
            .or_else(|| named(sessions.registry_name(session_id)))
            .or_else(|| named(Some(folder)))
            .unwrap_or_else(|| record.cwd.clone());
        Some(SessionView {
            id: record.id.clone(),
            label,
            cwd: record.cwd.clone(),
            cwd_label: home_relative(&record.cwd, &self.config.home.to_string_lossy(), true),
            status: sessions.status(session_id).unwrap_or(ended),
            error: sessions.error(session_id),
        })
    }

    /// A change to a window's layout, as the window asks for it.
    pub fn dispatch(&mut self, window_id: &str, action: &LayoutAction, cx: &mut Context<Self>) {
        let Some(window) = self.state.windows.iter_mut().find(|window| window.id == window_id) else {
            return;
        };
        *window = apply_layout_action(window, action);
        // A separator being dragged: written once it rests.
        if matches!(action, LayoutAction::SplitResize { .. }) {
            self.save_later(cx);
        } else {
            self.save(cx);
        }
        cx.emit(AppEvent::Window(window_id.to_owned()));
    }
}
