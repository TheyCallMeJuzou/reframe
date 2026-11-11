//! Persisted application state (`state.json`) and its schema.
//!
//! Ported from the Electron version's `src/shared/state.ts` (and the node types of
//! `src/shared/layout.ts`). The file format is shared with reframe 0.x: version 1, the same keys,
//! the same validation. A file either version wrote must load in the other, so nothing here may
//! add, rename or re-type a key without a new format version on both sides.
//!
//! As in the Electron version, unknown keys are dropped on load and a single invalid value makes
//! the whole file invalid (the store then sets it aside and uses the backup).

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const STATE_VERSION: u32 = 1;

/// Values accepted by `claude --permission-mode`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    AcceptEdits,
    Auto,
    BypassPermissions,
    Manual,
    DontAsk,
    Plan,
}

pub const PERMISSION_MODES: [PermissionMode; 6] = [
    PermissionMode::AcceptEdits,
    PermissionMode::Auto,
    PermissionMode::BypassPermissions,
    PermissionMode::Manual,
    PermissionMode::DontAsk,
    PermissionMode::Plan,
];

impl PermissionMode {
    /// The value as `claude --permission-mode` takes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AcceptEdits => "acceptEdits",
            Self::Auto => "auto",
            Self::BypassPermissions => "bypassPermissions",
            Self::Manual => "manual",
            Self::DontAsk => "dontAsk",
            Self::Plan => "plan",
        }
    }
}

/// Models offered for `--model` (any other name Claude Code knows works too).
pub const MODEL_SUGGESTIONS: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// `Row`: children side by side. `Col`: children stacked.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    Row,
    Col,
}

/// A window's layout: a tree of splits, groups (a named, coloured frame around a subtree) and
/// tiles (one session each).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LayoutNode {
    Tile(TileNode),
    Split(SplitNode),
    Group(GroupNode),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TileNode {
    pub session_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SplitNode {
    pub id: String,
    pub dir: SplitDir,
    pub children: Vec<LayoutNode>,
    /// Percentages that sum to 100, one per child.
    #[serde(serialize_with = "js::numbers")]
    pub sizes: Vec<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GroupNode {
    pub id: String,
    pub name: String,
    pub color: String,
    pub child: Box<LayoutNode>,
}

/// Remembers where a removed tile sat, so it can be put back there.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RestoreHint {
    /// Key of the neighbour the tile was next to.
    pub anchor: String,
    pub side: Side,
    /// The tile's share of the space it shared with the anchor (0..1).
    #[serde(serialize_with = "js::number")]
    pub ratio: f64,
    /// The tile's share of its whole split (0..1), restored when it goes back into a split of
    /// that direction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "js::optional_number"
    )]
    pub share: Option<f64>,
}

/// A collapsed tile, or a whole collapsed group with its layout.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DockEntry {
    #[serde(rename_all = "camelCase")]
    Tile {
        session_id: String,
        hint: Option<RestoreHint>,
    },
    Group {
        #[serde(with = "group_with_kind")]
        node: GroupNode,
        hint: Option<RestoreHint>,
    },
}

/// A window's position and size, in device-independent pixels as Electron reports them.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    #[serde(serialize_with = "js::number")]
    pub x: f64,
    #[serde(serialize_with = "js::number")]
    pub y: f64,
    #[serde(serialize_with = "js::number")]
    pub width: f64,
    #[serde(serialize_with = "js::number")]
    pub height: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub id: String,
    pub bounds: Option<Bounds>,
    pub maximized: bool,
    /// Full screen (F11); `bounds` stay the window's normal size meanwhile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<bool>,
    pub hidden: bool,
    pub layout: Option<LayoutNode>,
    pub dock: Vec<DockEntry>,
    /// Whether the session list is shown in this window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_open: Option<bool>,
    /// The workspace this window was saved as or opened from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_name: Option<String>,
    /// Claude Code made the worktree, and the session runs in it: starting it fresh again must
    /// not make another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_created: Option<bool>,
}

/// `Open`: should run (resumed on start). `Ended`: kept in the list but not running.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    Open,
    Ended,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    /// The Claude Code session id; changes on /clear, so it is updated while the session runs.
    pub claude_session_id: String,
    /// The session's folder; for a session in WSL in the form `\\wsl.localhost\<distro>\...`.
    pub cwd: String,
    /// The name reframe shows; without one, Claude Code's name for the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The name Claude Code gets (`--name`) on every start, so it keeps it when resumed.
    /// `None`: absent, which is the case until the session is renamed in reframe; until then it
    /// is the same as `name`. `Some(None)`: no name. Renaming changes only reframe's name.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub claude_name: Option<Option<String>>,
    pub launch: LaunchOptions,
    pub lifecycle: Lifecycle,
    /// Milliseconds since the epoch.
    #[serde(serialize_with = "js::number")]
    pub created_at: f64,
    /// When it was last used: a prompt, or Claude done or asking. Not when it was started.
    /// Milliseconds since the epoch; may carry a fraction (taken from a file time).
    #[serde(serialize_with = "js::number")]
    pub last_active_at: f64,
    /// Last known process id, used to clean up orphans after a crash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// In WSL: the last known Linux process id of its claude (`pid` is wsl.exe's), for the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux_pid: Option<u32>,
    /// Whether the current `claude_session_id` has had a prompt. Claude Code writes a transcript
    /// only from the first message on, so an unused session cannot be resumed and is started
    /// fresh under the same id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    En,
    De,
}

/// The terminals' font size in pixels: what it starts at, and how far it goes.
pub const FONT_SIZE_NORMAL: u8 = 13;
pub const FONT_SIZE_MIN: u8 = 8;
pub const FONT_SIZE_MAX: u8 = 32;

/// What closing a window with sessions in it does: ask; end them (the window goes, the first one
/// stays empty); or keep them running, the window hidden in the tray.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CloseWindow {
    #[default]
    Ask,
    End,
    Keep,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    /// "New session" fills the name with the folder's name.
    pub name_from_folder: bool,
    /// The UI language: the system's, or one chosen in Settings.
    #[serde(default)]
    pub language: Language,
    /// The terminals' font size, the same in every window.
    #[serde(default = "font_size_normal", deserialize_with = "font_size")]
    pub font_size: u8,
    /// Separators snap to a grid over the layout while they are dragged (Alt drags freely).
    #[serde(default = "yes")]
    pub snap_to_grid: bool,
    /// Asks GitHub for a newer version now and then (where this copy updates at all).
    #[serde(default = "yes")]
    pub check_for_updates: bool,
    #[serde(default)]
    pub close_window: CloseWindow,
    /// The claude to run, chosen in Settings; without one, the one on the PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_bin: Option<String>,
    /// What "New session" fills in for the model and the permission mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_permission_mode: Option<PermissionMode>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            name_from_folder: true,
            language: Language::System,
            font_size: FONT_SIZE_NORMAL,
            snap_to_grid: true,
            check_for_updates: true,
            close_window: CloseWindow::Ask,
            claude_bin: None,
            default_model: None,
            default_permission_mode: None,
        }
    }
}

/// The part of a window a workspace keeps.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWindow {
    pub bounds: Option<Bounds>,
    pub maximized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_open: Option<bool>,
    pub layout: Option<LayoutNode>,
    pub dock: Vec<DockEntry>,
}

/// A window saved under a name, to be opened again later as a window of its own.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[serde(serialize_with = "js::number")]
    pub saved_at: f64,
    pub window: WorkspaceWindow,
    /// Its sessions as they were, so they can be resumed even after they have left the session
    /// list.
    pub sessions: Vec<SessionRecord>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: u32,
    /// By session id, in the order they were added.
    pub sessions: IndexMap<String, SessionRecord>,
    pub windows: Vec<WindowState>,
    pub recent_folders: Vec<String>,
    // Added after the first release of the format; older files get the defaults.
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    /// WSL distributions Claude Code was found in, so the session list can tell when one of them
    /// is not running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wsl_distros: Option<Vec<String>>,
}

fn yes() -> bool {
    true
}

fn font_size_normal() -> u8 {
    FONT_SIZE_NORMAL
}

/// A whole number within the allowed sizes, however the JSON spells it (13 or 13.0).
fn font_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let value = f64::deserialize(deserializer)?;
    let allowed = f64::from(FONT_SIZE_MIN)..=f64::from(FONT_SIZE_MAX);
    if value.fract() != 0.0 || !allowed.contains(&value) {
        return Err(serde::de::Error::custom(format!(
            "fontSize {value} is not a whole number from 8 to 32"
        )));
    }
    Ok(value as u8)
}

/// For a key that may be absent, null or a value: a key that is there yields `Some`, also for
/// null.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// A group written with its `"kind": "group"`, where the schema holds a group and nothing else.
mod group_with_kind {
    use super::*;

    pub fn serialize<S: Serializer>(node: &GroupNode, serializer: S) -> Result<S::Ok, S::Error> {
        LayoutNode::Group(node.clone()).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<GroupNode, D::Error> {
        match LayoutNode::deserialize(deserializer)? {
            LayoutNode::Group(node) => Ok(node),
            _ => Err(serde::de::Error::custom("a docked group must be a group node")),
        }
    }
}

/// Numbers the way JavaScript's `JSON.stringify` writes them: a whole number without a fraction
/// (`1280`, not `1280.0`). Keeps a file written here the same as one written by reframe 0.x.
mod js {
    use serde::Serializer;
    use serde::ser::SerializeSeq;

    pub fn number<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        // Up to 2^53 every whole number is exact in a double, and JavaScript prints it in full.
        if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
            serializer.serialize_i64(*value as i64)
        } else {
            serializer.serialize_f64(*value)
        }
    }

    pub fn optional_number<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => number(value, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn numbers<S: Serializer>(values: &[f64], serializer: S) -> Result<S::Ok, S::Error> {
        struct Js(f64);
        impl serde::Serialize for Js {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                number(&self.0, serializer)
            }
        }
        let mut seq = serializer.serialize_seq(Some(values.len()))?;
        for value in values {
            seq.serialize_element(&Js(*value))?;
        }
        seq.end()
    }
}

/// Why a state file cannot be used.
#[derive(Debug, PartialEq, Eq)]
pub enum StateError {
    /// Written by a newer reframe: left alone, never overwritten.
    Newer(u64),
    /// Not JSON, or not this format.
    Invalid(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Newer(version) => write!(f, "written by a newer version of reframe (format {version})"),
            Self::Invalid(reason) => write!(f, "not a valid state file: {reason}"),
        }
    }
}

impl std::error::Error for StateError {}

impl AppState {
    /// Reads a state file's text. Checks what the schema of reframe 0.x checks.
    pub fn parse(text: &str) -> Result<AppState, StateError> {
        let invalid = |err: serde_json::Error| StateError::Invalid(err.to_string());
        let value: serde_json::Value = serde_json::from_str(text).map_err(invalid)?;
        // Any number above this version counts as newer, as in reframe 0.x (also 1.5 or 2.0).
        if let Some(version) = value.get("version").and_then(serde_json::Value::as_f64)
            && version > f64::from(STATE_VERSION)
        {
            return Err(StateError::Newer(version.ceil() as u64));
        }
        let state: AppState = serde_json::from_value(value).map_err(invalid)?;
        if state.version != STATE_VERSION {
            return Err(StateError::Invalid(format!("format version {}", state.version)));
        }
        let windows = state.windows.iter().map(|window| (&window.layout, &window.dock));
        let saved = state
            .workspaces
            .iter()
            .map(|workspace| (&workspace.window.layout, &workspace.window.dock));
        for (layout, dock) in windows.chain(saved) {
            let docked = dock.iter().filter_map(|entry| match entry {
                DockEntry::Group { node, .. } => Some(node.child.as_ref()),
                DockEntry::Tile { .. } => None,
            });
            if !layout.iter().chain(docked).all(splits_have_two_children) {
                return Err(StateError::Invalid("a split with fewer than two children".into()));
            }
        }
        Ok(state)
    }

    /// The file's text: JSON indented by two spaces, as reframe 0.x writes it.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("the state is plain data")
    }
}

fn splits_have_two_children(node: &LayoutNode) -> bool {
    match node {
        LayoutNode::Tile(_) => true,
        LayoutNode::Group(group) => splits_have_two_children(&group.child),
        LayoutNode::Split(split) => split.children.len() >= 2 && split.children.iter().all(splits_have_two_children),
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

impl WindowState {
    pub fn empty(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            bounds: None,
            maximized: false,
            fullscreen: None,
            hidden: false,
            layout: None,
            dock: Vec::new(),
            sidebar_open: None,
            workspace_id: None,
        }
    }
}

impl Default for AppState {
    /// The state of a first start: one empty window.
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            sessions: IndexMap::new(),
            windows: vec![WindowState::empty(uuid::Uuid::new_v4().to_string())],
            recent_folders: Vec::new(),
            preferences: Preferences::default(),
            workspaces: Vec::new(),
            wsl_distros: None,
        }
    }
}
