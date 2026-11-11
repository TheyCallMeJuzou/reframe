//! reframe without a window: the persisted state, the layout model and the bookkeeping around
//! Claude Code sessions. Ported module by module from the Electron version (reframe 0.x); each
//! module names the file it comes from.

pub mod state;

pub mod actions;
pub mod attention;
pub mod groups;
pub mod last_used;
pub mod layout;
pub mod workspaces;

pub mod i18n;
pub mod links;
pub mod wsl;

pub mod store;

pub mod claude;

// Being ported side by side: each group builds on its own until they are done.
#[cfg(feature = "infra")]
pub mod cleanup;
#[cfg(feature = "infra")]
pub mod hooks;
#[cfg(feature = "infra")]
pub mod log;
#[cfg(feature = "infra")]
pub mod paths;

#[cfg(feature = "discovery")]
pub mod discovery;

pub mod session_logic;
