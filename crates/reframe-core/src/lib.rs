//! reframe without a window: the persisted state, the layout model and the bookkeeping around
//! Claude Code sessions. Ported module by module from the Electron version (reframe 0.x); each
//! module names the file it comes from.

pub mod state;

pub mod actions;
pub mod attention;
pub mod geometry;
pub mod groups;
pub mod last_used;
pub mod layout;
pub mod workspaces;

pub mod i18n;
pub mod links;
pub mod wsl;

pub mod store;

pub mod claude;

pub mod cleanup;
pub mod hooks;
pub mod log;
pub mod paths;

pub mod discovery;

pub mod session_logic;
