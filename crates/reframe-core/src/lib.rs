//! reframe without a window: the persisted state, the layout model and the bookkeeping around
//! Claude Code sessions. Ported module by module from the Electron version (reframe 0.x); each
//! module names the file it comes from.

pub mod state;

// While the modules are being ported side by side, each group builds on its own.
#[cfg(feature = "layout")]
pub mod actions;
#[cfg(feature = "layout")]
pub mod attention;
#[cfg(feature = "layout")]
pub mod groups;
#[cfg(feature = "layout")]
pub mod last_used;
#[cfg(feature = "layout")]
pub mod layout;
#[cfg(feature = "layout")]
pub mod workspaces;

#[cfg(feature = "text")]
pub mod i18n;
#[cfg(feature = "text")]
pub mod links;
#[cfg(feature = "text")]
pub mod wsl;

#[cfg(feature = "store")]
pub mod store;
