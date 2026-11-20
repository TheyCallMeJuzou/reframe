//! Command line for starting or resuming a session.
//!
//! Ported from the Electron version's `src/main/claude/args.ts`.

use crate::state::SessionRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchMode {
    New,
    Resume,
}

/// `settings_file` brings reframe's hooks; without one (WSL, out of the hooks' reach) the status
/// comes from the registry.
pub fn claude_args(session: &SessionRecord, mode: LaunchMode, settings_file: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = match mode {
        LaunchMode::New => vec!["--session-id".into(), session.claude_session_id.clone()],
        LaunchMode::Resume => vec!["--resume".into(), session.claude_session_id.clone()],
    };
    let name = match &session.claude_name {
        None => session.name.as_deref(),
        Some(name) => name.as_deref(),
    };
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        args.extend(["--name".into(), name.to_owned()]);
    }
    if let Some(model) = session.launch.model.as_deref().filter(|model| !model.is_empty()) {
        args.extend(["--model".into(), model.to_owned()]);
    }
    if let Some(mode) = session.launch.permission_mode {
        args.extend(["--permission-mode".into(), mode.as_str().to_owned()]);
    }
    // A worktree session that was started once runs in its worktree folder; -w again would make
    // another one in it.
    let worktree = session.launch.worktree == Some(true) && session.launch.worktree_created != Some(true);
    if mode == LaunchMode::New && worktree {
        args.push("--worktree".into());
        if let Some(name) = session.launch.worktree_name.as_deref().filter(|name| !name.is_empty()) {
            args.push(name.to_owned());
        }
    }
    if let Some(file) = settings_file.filter(|file| !file.is_empty()) {
        args.extend(["--settings".into(), file.to_owned()]);
    }
    args
}
