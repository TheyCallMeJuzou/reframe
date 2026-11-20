//! Starting Claude Code and reading what it leaves on disk.
//!
//! Ported from the Electron version's `src/main/claude/`. The functions here do no process
//! management themselves: whether a process is alive and when it started are passed in, so this
//! crate stays free of platform code and the tests can say what the system would.

pub mod args;
pub mod env;
pub mod registry;
pub mod resolve;
pub mod settings;

#[cfg(test)]
mod tests;
