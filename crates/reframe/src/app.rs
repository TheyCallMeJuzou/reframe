//! Wires state, sessions and hooks together. The controller owns the state: windows send intents,
//! it applies them, saves, and tells the windows what changed.
//!
//! Ported from the Electron version's `src/main/app.ts`, as far as sessions go. What follows with
//! later milestones is named where it would be: asking before a conversation that runs elsewhere
//! is taken over, sessions in WSL, the tray, the session list, the system going down.

mod list;
mod open;
mod terminal;
mod update;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::FutureExt;
use futures::channel::mpsc::unbounded;
use futures::channel::oneshot;
use futures::future::Shared;
use gpui_kit::*;
use reframe_core::actions::{
    LayoutAction, add_tile, apply_layout_action, drop_session, is_window_empty, patch_group, put_node, take_node,
    window_groups, window_has_session, window_sessions, windows_to_restore,
};
use reframe_core::attention::next_waiting;
use reframe_core::claude::args::LaunchMode;
use reframe_core::claude::registry::claude_home;
use reframe_core::claude::resolve::{self, ClaudeBinary, NodeRunner, Platform, ResolveOptions, resolve_claude};
use reframe_core::cleanup::{clean_up, drops_dir};
use reframe_core::discovery::TranscriptIndex;
use reframe_core::groups::{is_group_color, next_group_color, next_group_number};
use reframe_core::hooks::HookServer;
use reframe_core::i18n::{Locale, locale_of, resolve_locale, t};
use reframe_core::layout::{
    DropTarget, GroupPatch, NewGroup, add_to_group, dissolve_group, group_tile, leaves, remove_from_group,
};
use reframe_core::log::Logger;
use reframe_core::paths::home_relative;
use reframe_core::session_list::SessionList;
use reframe_core::session_logic::{IdentityPatch, NotifyKind, SessionError, SessionStatus};
use reframe_core::shortcuts::{TextSizeChange, font_size_after};
use reframe_core::state::{
    AppState, CloseWindow, DockEntry, GroupNode, LaunchOptions, Lifecycle, PermissionMode, Preferences, SessionRecord,
    WindowState,
};
use reframe_core::store::{LoadSource, SAVE_DELAY, StateStore};
use reframe_core::wsl::parse_wsl_path;
use serde_json::json;
use uuid::Uuid;

use crate::sessions::{SessionEvent, SessionManager, SessionManagerDeps};
use crate::terminal_view::TerminalFontSize;
use crate::wsl::WslDistros;

const MAX_RECENT_FOLDERS: usize = 8;
/// How long a session gets to end on Ctrl+C before its process tree is killed.
const STOP_MS: Duration = Duration::from_millis(5000);
/// How long after the system said it is going down sessions that end still count as ended by
/// it.
const SYSTEM_ENDING_MS: Duration = Duration::from_millis(60_000);
/// The profile's folder under `%APPDATA%`. The native app keeps one of its own until it takes
/// over from the Electron version (1.0), whose folder is "reframe".
const PROFILE_FOLDER: &str = "reframe-native";
/// What Windows knows reframe by: its notifications, its taskbar buttons, its entry among the
/// programs started when the user logs in. The installed 0.x is "dev.reframe.app"; the native
/// app takes that over with the profile.
pub const APP_ID: &str = "dev.reframe.native";
/// The uninstaller reframe's installer puts next to it (named as the Electron version's was).
pub const UNINSTALLER: &str = "Uninstall reframe.exe";
/// Marks a start by the system: reframe then stays in the tray.
pub const AUTOSTART_ARG: &str = "--autostart";
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
    pub copy: CopyKind,
    /// What Windows knows this copy by (see [`APP_ID`]); a portable copy has one of its own.
    pub app_id: String,
}

/// What kind of copy of reframe this is: it decides where the profile is and what the copy does
/// about updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKind {
    /// A debug build (`cargo run`).
    Development,
    /// A release build that was not installed.
    Release,
    /// Installed by reframe's installer (its uninstaller is next to it): it updates itself.
    Installed,
    /// The portable copy: a folder "data" next to it holds its profile, so the folder can move
    /// and runs apart from an installed reframe. It only tells about updates.
    Portable,
}

impl CopyKind {
    /// What a copy at `exe` is.
    pub fn of(exe: &Path) -> CopyKind {
        if cfg!(debug_assertions) {
            return CopyKind::Development;
        }
        let folder = exe.parent().unwrap_or(Path::new(""));
        if folder.join("data").is_dir() {
            CopyKind::Portable
        } else if folder.join(UNINSTALLER).is_file() {
            CopyKind::Installed
        } else {
            CopyKind::Release
        }
    }

    /// As a second start tells it to the running reframe (see [`crate::win::TrayEvent::Reveal`]).
    pub fn build(self) -> usize {
        if self == CopyKind::Development { 1 } else { 2 }
    }
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
    /// keeps a profile of its own ([`PROFILE_FOLDER`]), so the two never write the same state
    /// file.
    pub fn from_environment() -> Config {
        let env: HashMap<String, String> = std::env::vars().collect();
        let set = |name: &str| env.get(name).filter(|value| !value.is_empty()).cloned();
        let home = PathBuf::from(set("USERPROFILE").unwrap_or_default());
        let exe = std::env::current_exe().unwrap_or_default();
        let copy = CopyKind::of(&exe);
        let portable_data = exe.parent().unwrap_or(Path::new("")).join("data");
        // Tests and side-by-side runs use a profile of their own; the portable copy keeps it next
        // to itself.
        let user_data = match set("REFRAME_USER_DATA") {
            Some(dir) => PathBuf::from(dir),
            None if copy == CopyKind::Portable => portable_data.clone(),
            None => set("APPDATA")
                .map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
                .join(PROFILE_FOLDER),
        };
        // Windows would put a portable copy's windows and notifications with the installed one's.
        let app_id = if copy == CopyKind::Portable {
            let key = crate::win::profile_key(&portable_data);
            format!("{APP_ID}.portable.{}", key.trim_start_matches("reframe-"))
        } else {
            APP_ID.to_owned()
        };
        let e2e = set("REFRAME_E2E").is_some();
        let system_locale = set("REFRAME_LOCALE").unwrap_or_else(windows_locale);
        Config {
            user_data,
            env,
            home,
            e2e,
            system_locale,
            copy,
            app_id,
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

pub enum AppEvent {
    /// Something about a session changed that its tile shows.
    Session(String),
    /// A window's layout changed.
    Window(String),
    /// There is a new window to open.
    WindowOpened(String),
    /// A window is gone for good.
    WindowClosed(String),
    /// A window was closed by the user and stays: it goes out of sight, its sessions run on.
    WindowPutAway(String),
    /// A window comes to the front, and one of its sessions gets the keyboard.
    FocusTile { window: String, session: String },
    /// A window was hidden or shown.
    Visibility,
    /// A session is done, or needs the user.
    Notify(String, NotifyKind),
    /// A preference changed that windows show (the language, the text size, …).
    Preferences,
    /// The session list's first page changed (see [`Controller::first_page`]).
    SessionList,
    /// Workspaces were saved, renamed, opened or deleted.
    Workspaces,
    /// A window is to come to the front.
    ShowWindow(String),
    /// An update was downloaded; it is installed on quit or with "Restart to update".
    UpdateReady(String),
    /// A newer version is out, for the portable copy to download by hand.
    UpdateAvailable(String),
}

pub use list::local_time;

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
    /// Set for a while after the system said it goes down: sessions that end meanwhile were
    /// ended by it, not by the user.
    system_ending: Option<Task<()>>,
    ended_while_system_ending: HashSet<String>,
    /// Whether Windows starts reframe when the user logs in.
    starts_with_system: bool,
    /// The write that follows frequent changes after a moment.
    save_soon: Option<Task<()>>,
    claude_home: PathBuf,
    /// Past sessions in Claude Code's transcripts, listed once and then as they change.
    transcripts: Arc<Mutex<TranscriptIndex>>,
    /// Claudes running outside reframe (with a folder), as last looked at; `None` before the
    /// first look.
    outside: Option<list::OutsideScan>,
    /// When WSL was last looked at, and the transcript indexes of its distributions.
    wsl_looked_at: Option<std::time::Instant>,
    wsl_transcripts: HashMap<PathBuf, Arc<Mutex<TranscriptIndex>>>,
    /// A look at what runs outside reframe is under way; another one follows it (`rescan`, and
    /// in WSL too after `rescan_wsl`).
    scanning: bool,
    rescan: bool,
    rescan_wsl: Option<Duration>,
    scan_waiters: Vec<oneshot::Sender<()>>,
    /// The list's first page as the windows got it last.
    list_sent: Option<Rc<SessionList>>,
    /// The windows get the list in a moment.
    list_push: bool,
    /// The watcher and the regular look.
    list_tasks: Vec<Task<()>>,
    /// Conversations being adopted from the list (by claude session id): asked for twice, it is
    /// one adoption.
    adopting: HashMap<String, Shared<Task<Option<SessionRecord>>>>,
    /// Sessions whose start waits for an answer (whether to take one over that runs elsewhere).
    starts_pending: HashSet<String>,
    /// WSL distributions, and running claude in them.
    wsl: Arc<WslDistros>,
    /// Terminal windows whose drop is being handled (a second report of it is ignored).
    taking_over: HashSet<isize>,
    updates: update::Updates,
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
        let transcripts = Arc::new(Mutex::new(TranscriptIndex::new(claude_home.clone())));
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

        let wsl = Arc::new(WslDistros::new(&config.env, log.clone()));
        let deps = SessionManagerDeps {
            wsl: wsl.clone(),
            claude_home: claude_home.clone(),
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
            SessionEvent::Changed(id) => {
                cx.emit(AppEvent::Session(id.clone()));
                // Its status shows in the list.
                this.list_changed(cx);
            }
            SessionEvent::Identity(id, patch) => this.update_identity(id, patch, cx),
            SessionEvent::Notify(id, kind) => cx.emit(AppEvent::Notify(id.clone(), *kind)),
            SessionEvent::Used(id) => this.session_used(id, cx),
            SessionEvent::Ended(id) => this.session_ended(id, cx),
        });

        // The end-to-end tests leave the system alone.
        let starts_with_system =
            !config.e2e && crate::win::registry_text(crate::win::RUN_KEY, &config.app_id).is_some();
        let mut controller = Self {
            claude_home,
            transcripts,
            outside: None,
            wsl_looked_at: None,
            wsl_transcripts: HashMap::new(),
            scanning: false,
            rescan: false,
            rescan_wsl: None,
            scan_waiters: Vec::new(),
            list_sent: None,
            list_push: false,
            list_tasks: Vec::new(),
            adopting: HashMap::new(),
            starts_pending: HashSet::new(),
            wsl,
            taking_over: HashSet::new(),
            updates: update::Updates::new(&config),
            starts_with_system,
            system_ending: None,
            ended_while_system_ending: HashSet::new(),
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
        };
        controller.watch_list(cx);
        Ok(controller)
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

    pub fn log(&self) -> &Logger {
        &self.log
    }

    pub fn wsl(&self) -> &Arc<WslDistros> {
        &self.wsl
    }

    /// Where a folder is in WSL, if it is in a distribution.
    pub fn wsl_place(&self, folder: &str) -> Option<reframe_core::wsl::WslPath> {
        parse_wsl_path(folder, Some(&self.wsl.share))
    }

    /// How many of a window's sessions need the user.
    pub fn waiting_in(&self, window: &WindowState, cx: &App) -> usize {
        let sessions = self.sessions.read(cx);
        window_sessions(window)
            .iter()
            .filter(|id| sessions.status(id) == Some(SessionStatus::Waiting))
            .count()
    }

    /// How many sessions need the user, in all windows.
    pub fn waiting_total(&self, cx: &App) -> usize {
        self.state
            .windows
            .iter()
            .map(|window| self.waiting_in(window, cx))
            .sum()
    }

    /// Whether Windows starts reframe when the user logs in.
    pub fn starts_with_system(&self) -> bool {
        self.starts_with_system
    }

    /// Has Windows start reframe when the user logs in (in the tray, see [`AUTOSTART_ARG`]), or
    /// no longer.
    pub fn set_start_with_system(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let command = std::env::current_exe()
            .map(|exe| format!("\"{}\" {AUTOSTART_ARG}", exe.display()))
            .map_err(|error| error.to_string());
        let changed = if self.config.e2e {
            Ok(())
        } else {
            command.and_then(|command| {
                let command = enabled.then_some(command.as_str());
                crate::win::set_registry_text(crate::win::RUN_KEY, &self.config.app_id, command)
            })
        };
        match changed {
            Ok(()) => {
                self.starts_with_system = enabled;
                let message = if enabled {
                    "starts with the system now"
                } else {
                    "no longer starts with the system"
                };
                self.log.info(message, None);
            }
            Err(error) => self.log.warn(
                "could not change starting with the system",
                Some(json!({ "error": error })),
            ),
        }
        cx.emit(AppEvent::Preferences);
    }

    /// A window was hidden (closed into the tray) or shown again: remembered, so that it stays
    /// that way over a restart.
    pub fn set_window_hidden(&mut self, window_id: &str, hidden: bool, cx: &mut Context<Self>) {
        let Some(window) = self.state.windows.iter_mut().find(|window| window.id == window_id) else {
            return;
        };
        if window.hidden != hidden {
            window.hidden = hidden;
            self.save_later(cx);
        }
        cx.emit(AppEvent::Visibility);
        // A secondary window hidden while it is empty is gone for good, as when its last tile
        // leaves it.
        if hidden {
            self.close_if_empty(window_id, cx);
        }
    }

    /// Brings a session up: its window shown and in front, its tile back from the strip, and
    /// the keyboard in it.
    pub fn focus_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(window) = self.window_of(session_id) else {
            return;
        };
        let window_id = window.id.clone();
        let collapsed = window.dock.iter().find_map(|entry| match entry {
            DockEntry::Tile { session_id: id, .. } if id == session_id => {
                Some(LayoutAction::TileExpand { session_id: id.clone() })
            }
            DockEntry::Group { node, .. } if leaves(Some(&node.child)).iter().any(|id| id == session_id) => {
                Some(LayoutAction::GroupExpand {
                    group_id: node.id.clone(),
                })
            }
            _ => None,
        });
        if let Some(action) = collapsed {
            self.dispatch(&window_id, &action, cx);
        }
        cx.emit(AppEvent::FocusTile {
            window: window_id,
            session: session_id.to_owned(),
        });
    }

    /// Brings up the next session that needs the user: in this window first (after `after`),
    /// then in the others.
    pub fn focus_next_waiting(&mut self, window_id: &str, after: Option<&str>, cx: &mut Context<Self>) {
        let here = self.state.windows.iter().filter(|window| window.id == window_id);
        let others = self.state.windows.iter().filter(|window| window.id != window_id);
        let order: Vec<String> = here.chain(others).flat_map(window_sessions).collect();
        let sessions = self.sessions.read(cx);
        let waiting: HashSet<String> = order
            .iter()
            .filter(|id| sessions.status(id) == Some(SessionStatus::Waiting))
            .cloned()
            .collect();
        if let Some(next) = next_waiting(&order, &waiting, after).map(str::to_owned) {
            self.focus_session(&next, cx);
        }
    }

    /// The system is shutting down, restarting or logging the user off. Sessions it ends now
    /// were not ended by the user: they stay open and resume on the next start. Should the
    /// system not go down after all (another program objected), the ones that ended meanwhile
    /// count as ended a minute later.
    pub fn system_ending(&mut self, cx: &mut Context<Self>) {
        if self.system_ending.is_none() {
            self.log
                .info("the system is going down, sessions it ends stay open", None);
        }
        self.system_ending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SYSTEM_ENDING_MS).await;
            let _ = this.update(cx, |this, cx| {
                this.system_ending = None;
                for id in std::mem::take(&mut this.ended_while_system_ending) {
                    if !this.sessions.read(cx).is_running(&id) {
                        this.update_session(&id, true, cx, |record| record.lifecycle = Lifecycle::Ended);
                    }
                }
            });
        }));
        self.save(cx);
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
        let there = |folder: &&String| self.wsl_place(folder).is_some() || Path::new(folder.as_str()).is_dir();
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

    /// Where a window is now, to open it there again: its place and size when it is neither
    /// maximized nor full screen, and whether it is one of those.
    pub fn set_window_bounds(
        &mut self,
        window_id: &str,
        bounds: reframe_core::state::Bounds,
        maximized: bool,
        fullscreen: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(window) = self.state.windows.iter_mut().find(|window| window.id == window_id) else {
            return;
        };
        let fullscreen = fullscreen.then_some(true);
        if window.bounds.as_ref() == Some(&bounds) && window.maximized == maximized && window.fullscreen == fullscreen {
            return;
        }
        window.bounds = Some(bounds);
        window.maximized = maximized;
        window.fullscreen = fullscreen;
        self.save_later(cx);
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

    // ---------- windows ----------

    /// A new, empty window, a little offset from another one's place. It stays until something
    /// leaves it empty again (like a window a tile was torn out into) and comes back after a
    /// restart like any other.
    pub fn new_window(&mut self, near: Option<reframe_core::state::Bounds>, cx: &mut Context<Self>) -> String {
        let mut window = WindowState::empty(Uuid::new_v4().to_string());
        window.bounds = near.map(|near| reframe_core::state::Bounds {
            x: near.x + 40.0,
            y: near.y + 40.0,
            ..near
        });
        let id = window.id.clone();
        self.state.windows.push(window);
        self.save(cx);
        self.log.info("new window", Some(json!({ "window": id })));
        cx.emit(AppEvent::WindowOpened(id.clone()));
        id
    }

    /// Moves a tile or group from another window into this one; without a target it goes to
    /// the end. Its sessions run on.
    pub fn transfer_node(
        &mut self,
        to_id: &str,
        key: &str,
        from_id: &str,
        target: Option<&DropTarget>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(from) = self.find_window(from_id) else {
            return false;
        };
        if from_id == to_id || self.find_window(to_id).is_none() {
            return false;
        }
        let taken = take_node(from, key);
        let Some(node) = taken.node else { return false };
        let first = leaves(Some(&node)).into_iter().next();
        for window in &mut self.state.windows {
            if window.id == from_id {
                *window = taken.win.clone();
            } else if window.id == to_id {
                *window = put_node(window, node.clone(), target);
            }
        }
        self.save(cx);
        self.log.info(
            "moved to another window",
            Some(json!({ "node": key, "from": from_id, "to": to_id })),
        );
        cx.emit(AppEvent::Window(from_id.to_owned()));
        cx.emit(AppEvent::Window(to_id.to_owned()));
        self.close_if_empty(from_id, cx);
        if let Some(session) = first {
            cx.emit(AppEvent::FocusTile {
                window: to_id.to_owned(),
                session,
            });
        }
        true
    }

    /// Gives a tile or group a window of its own at a point of the screen. `size` is how big
    /// it was where it came from. Whether the point is free of reframe's windows is for the
    /// caller to say. The new window's id, if there is one.
    pub fn tear_out(
        &mut self,
        from_id: &str,
        key: &str,
        point: (f64, f64),
        size: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let taken = take_node(self.find_window(from_id)?, key);
        let node = taken.node?;
        let (width, height) = size.unwrap_or((800.0, 500.0));
        let mut window = WindowState::empty(Uuid::new_v4().to_string());
        // Room for the window's toolbar and padding around the tile; the title bar lands under
        // the pointer.
        window.bounds = Some(reframe_core::state::Bounds {
            x: (point.0 - 60.0).round(),
            y: (point.1 - 16.0).round(),
            width: (width + 24.0).clamp(480.0, 2400.0).round(),
            height: (height + 90.0).clamp(320.0, 1600.0).round(),
        });
        window.layout = Some(node);
        let id = window.id.clone();
        for window in &mut self.state.windows {
            if window.id == from_id {
                *window = taken.win.clone();
            }
        }
        self.state.windows.push(window);
        self.save(cx);
        self.log.info(
            "torn out into a new window",
            Some(json!({ "node": key, "from": from_id, "window": id })),
        );
        cx.emit(AppEvent::Window(from_id.to_owned()));
        cx.emit(AppEvent::WindowOpened(id.clone()));
        self.close_if_empty(from_id, cx);
        Some(id)
    }

    /// What closing a window does without a question: nothing is asked for an empty window or
    /// when an answer was kept. `None`: the user is asked.
    pub fn close_choice(&self, window_id: &str) -> Option<CloseWindow> {
        let window = self.find_window(window_id)?;
        // While the system goes down nothing is asked.
        if window_sessions(window).is_empty() || self.system_ending.is_some() {
            return Some(CloseWindow::Keep);
        }
        match self.state.preferences.close_window {
            CloseWindow::Ask => None,
            choice => Some(choice),
        }
    }

    /// The user closes a window: its sessions are ended and it goes (the first one stays,
    /// empty), or they run on and the window is put away. An empty secondary window is gone for
    /// good. `remember`: this is the answer from now on.
    pub fn close_window(&mut self, window_id: &str, choice: CloseWindow, remember: bool, cx: &mut Context<Self>) {
        if remember && choice != CloseWindow::Ask {
            self.update_preferences(cx, |preferences| preferences.close_window = choice);
            let choice = serde_json::to_value(choice).unwrap_or_default();
            self.log.info(
                "closing windows with sessions from now on",
                Some(json!({ "choice": choice })),
            );
        }
        // What is in the window now (the question may have been open for a while).
        let Some(window) = self.find_window(window_id) else {
            return;
        };
        if choice == CloseWindow::End {
            let ending = window_sessions(window);
            self.log.info(
                "window closed, its sessions ended",
                Some(json!({ "window": window_id, "sessions": ending.len() })),
            );
            // Ending the last one closes a secondary window for good; the first one stays, empty.
            for id in ending {
                self.end_session(&id, cx);
            }
        }
        self.close_if_empty(window_id, cx);
        if self.find_window(window_id).is_some() {
            cx.emit(AppEvent::WindowPutAway(window_id.to_owned()));
        }
    }

    // ---------- groups ----------

    fn window_of_group(&self, group_id: &str) -> Option<&WindowState> {
        self.state
            .windows
            .iter()
            .find(|window| window_groups(window).iter().any(|group| group.id == group_id))
    }

    fn find_group(&self, group_id: &str) -> Option<&GroupNode> {
        self.state
            .windows
            .iter()
            .flat_map(window_groups)
            .find(|group| group.id == group_id)
    }

    /// A new group's name ("Group n") and the next free colour.
    fn new_group_look(&self) -> (String, String) {
        let locale = self.locale();
        let name_for = |n: u32| t(locale, "group.defaultName", &[("n", &n.to_string())]).unwrap_or_default();
        let existing: Vec<&GroupNode> = self.state.windows.iter().flat_map(window_groups).collect();
        let names: Vec<&str> = existing.iter().map(|group| group.name.as_str()).collect();
        let colors: Vec<&str> = existing.iter().map(|group| group.color.as_str()).collect();
        (
            name_for(next_group_number(&names, name_for)),
            next_group_color(&colors).to_owned(),
        )
    }

    /// Frames a tile as a new group, named "Group n" with the next free colour.
    pub fn create_group(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(window_id) = self.window_of(session_id).map(|window| window.id.clone()) else {
            return;
        };
        let (name, color) = self.new_group_look();
        self.log.info(
            "group created",
            Some(json!({ "session": session_id, "name": name, "color": color })),
        );
        let group = NewGroup { id: None, name, color };
        self.mutate_window(&window_id, cx, |window| WindowState {
            layout: group_tile(window.layout.as_ref(), session_id, group),
            ..window.clone()
        });
    }

    pub fn add_session_to_group(&mut self, session_id: &str, group_id: &str, cx: &mut Context<Self>) {
        let Some(window_id) = self.window_of(session_id).map(|window| window.id.clone()) else {
            return;
        };
        if self.window_of_group(group_id).map(|window| window.id.as_str()) != Some(window_id.as_str()) {
            return;
        }
        self.mutate_window(&window_id, cx, |window| WindowState {
            layout: add_to_group(window.layout.as_ref(), session_id, group_id),
            ..window.clone()
        });
    }

    pub fn remove_session_from_group(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(window_id) = self.window_of(session_id).map(|window| window.id.clone()) else {
            return;
        };
        self.mutate_window(&window_id, cx, |window| WindowState {
            layout: remove_from_group(window.layout.as_ref(), session_id),
            ..window.clone()
        });
    }

    /// Takes the frame away; its tiles stay where they are.
    pub fn dissolve_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let Some(window_id) = self.window_of_group(group_id).map(|window| window.id.clone()) else {
            return;
        };
        self.mutate_window(&window_id, cx, |window| WindowState {
            layout: dissolve_group(window.layout.as_ref(), group_id),
            ..window.clone()
        });
    }

    pub fn recolor_group(&mut self, group_id: &str, color: &str, cx: &mut Context<Self>) {
        let Some(window_id) = self.window_of_group(group_id).map(|window| window.id.clone()) else {
            return;
        };
        if !is_group_color(color) {
            return;
        }
        let patch = GroupPatch {
            name: None,
            color: Some(color.to_owned()),
        };
        self.mutate_window(&window_id, cx, |window| patch_group(window, group_id, &patch));
    }

    /// Ends every session of the group; like a tile's ×, they stay in the session list.
    pub fn end_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let Some(group) = self.find_group(group_id) else {
            return;
        };
        let sessions = leaves(Some(&group.child));
        self.log.info(
            "ending all sessions of a group",
            Some(json!({ "group": group_id, "sessions": sessions.len() })),
        );
        for session_id in sessions {
            self.end_session(&session_id, cx);
        }
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
            // On the start nobody can be asked: one whose conversation runs elsewhere now waits
            // for "Take over".
            if self.window_of(&id).is_some() {
                self.start_or_take_over(&id, LaunchMode::Resume, None, cx);
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

    fn save(&mut self, cx: &mut Context<Self>) {
        self.save_soon = None;
        if let Err(error) = self.store.save_now(&self.state) {
            self.log
                .error("state could not be saved", Some(json!({ "error": error.to_string() })));
        }
        self.list_changed(cx);
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
        self.list_changed(cx);
    }

    /// A session ended by itself (not by reframe): it stays in its tile as ended.
    fn session_ended(&mut self, session_id: &str, cx: &mut Context<Self>) {
        // Unless the system ended it on its way down.
        if self.system_ending.is_some() {
            self.ended_while_system_ending.insert(session_id.to_owned());
            self.log.info(
                "session ended while the system goes down, stays open",
                Some(json!({ "session": session_id })),
            );
            cx.emit(AppEvent::Session(session_id.to_owned()));
            return;
        }
        self.update_session(session_id, true, cx, |record| record.lifecycle = Lifecycle::Ended);
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
        // In a WSL folder claude runs in its distribution, which is asked when the session starts
        // (see `create_session_checked` for asking first).
        if self.wsl_place(&cwd).is_none() && find_claude(&self.config, self.claude_bin.borrow().as_deref()).is_none() {
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

    /// Like `create_session`, after looking off the UI thread whether the folder is there and claude
    /// can run in it: a WSL folder's distribution is asked (which starts it). The error names the
    /// distribution claude is missing in.
    pub fn create_session_checked(
        &mut self,
        window_id: &str,
        request: NewSessionRequest,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, (SessionError, Option<String>)>> {
        let cwd = request.cwd.trim().to_owned();
        let place = self.wsl_place(&cwd);
        let wsl = self.wsl.clone();
        let window_id = window_id.to_owned();
        cx.spawn(async move |this, cx| {
            let checked = cx
                .background_executor()
                .spawn(async move {
                    if !Path::new(&cwd).is_dir() {
                        return Err((SessionError::FolderMissing, None));
                    }
                    match place {
                        Some(place) if wsl.info(&place.distro).and_then(|info| info.claude).is_none() => {
                            Err((SessionError::ClaudeNotFound, Some(place.distro)))
                        }
                        _ => Ok(()),
                    }
                })
                .await;
            checked?;
            this.update(cx, |this, cx| {
                this.remember_wsl_distros(cx);
                this.create_session(&window_id, request, cx)
            })
            .map_err(|_| (SessionError::SpawnFailed, None))?
            .map_err(|error| (error, None))
        })
    }

    /// Distributions Claude Code was found in are remembered, so the list can tell when one of
    /// them is not running.
    fn remember_wsl_distros(&mut self, cx: &mut Context<Self>) {
        let found = self.wsl.take_found();
        if found.is_empty() {
            return;
        }
        let known = self.state.wsl_distros.get_or_insert_with(Vec::new);
        let mut changed = false;
        for distro in found {
            if !known.iter().any(|name| name.eq_ignore_ascii_case(&distro)) {
                known.push(distro);
                changed = true;
            }
        }
        if changed {
            self.save(cx);
        }
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
        let window_id = self.window_of(session_id).map(|window| window.id.clone());
        let mode = if fresh { LaunchMode::New } else { LaunchMode::Resume };
        self.start_or_take_over(session_id, mode, window_id, cx);
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
                this.wsl.release();
                if !ended {
                    let left = this.sessions.read(cx).active();
                    this.log.warn(
                        "sessions did not end in time, quitting anyway",
                        Some(json!({ "sessions": left })),
                    );
                }
                this.hooks.stop();
                this.save(cx);
                this.install_update_on_quit();
                this.log.info("reframe stopped", None);
            });
            crate::win::remove_tray_icon();
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
            cwd_label: match self.wsl_place(&record.cwd) {
                Some(place) => {
                    let path = match self.wsl.cached(&place.distro) {
                        Some(info) => home_relative(&place.path, &info.home, false),
                        None => place.path.clone(),
                    };
                    format!("{} {path}", place.distro)
                }
                None => home_relative(&record.cwd, &self.config.home.to_string_lossy(), true),
            },
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
