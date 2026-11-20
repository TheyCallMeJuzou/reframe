//! Environment for spawned claude processes.
//!
//! Ported from the Electron version's `src/main/claude/env.ts`.

use std::collections::HashMap;

/// Markers a running Claude Code session sets for its children. If reframe was started from
/// inside a Claude session, a spawned claude inheriting them becomes a "child session" that
/// writes no transcript and no registry entry. Only these markers are removed; other
/// `CLAUDE_CODE_*` variables are user config.
const PARENT_SESSION_MARKERS: [&str; 9] = [
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_ENTRYPOINT",
];

/// Whether a variable marks a parent Claude Code session (whatever its case).
pub fn is_parent_session_marker(name: &str) -> bool {
    PARENT_SESSION_MARKERS
        .iter()
        .any(|marker| marker.eq_ignore_ascii_case(name))
}

/// `base` without the parent-session markers, with `extra` on top.
pub fn claude_env(
    base: impl IntoIterator<Item = (String, String)>,
    extra: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = base
        .into_iter()
        .filter(|(name, _)| !is_parent_session_marker(name))
        .collect();
    env.extend(extra);
    env
}
