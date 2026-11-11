//! The decisions of the session manager, apart from the processes it runs: how to start a
//! session, what a hook or a registry entry means for it, how to read its exit.
//!
//! Ported from the Electron version's `src/main/sessions.ts`, where these are parts of the
//! `SessionManager` class. They are functions of their own here so they can be tested without a
//! pty; the manager in the app calls them in the same places.

use crate::claude::args::LaunchMode;
use crate::claude::registry::{RegistryEntry, RegistryStatus};
pub use crate::last_used::SessionStatus;
use crate::wsl::wsl_windows_path;

/// Why a session is not running, as the tile says it (`tile.error.<key>` in the texts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    ResumeFailed,
    TranscriptMissing,
    ClaudeNotFound,
    SpawnFailed,
    FolderMissing,
    HostCrashed,
    RunningElsewhere,
}

impl SessionError {
    pub fn key(self) -> &'static str {
        match self {
            Self::ResumeFailed => "resumeFailed",
            Self::TranscriptMissing => "transcriptMissing",
            Self::ClaudeNotFound => "claudeNotFound",
            Self::SpawnFailed => "spawnFailed",
            Self::FolderMissing => "folderMissing",
            Self::HostCrashed => "hostCrashed",
            Self::RunningElsewhere => "runningElsewhere",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyKind {
    Done,
    Waiting,
}

/// Persisted identity that changed: claude session id (after /clear), folder (worktrees), pid,
/// or first use. `None` leaves a field as it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IdentityPatch {
    pub claude_session_id: Option<String>,
    pub cwd: Option<String>,
    pub pid: Option<u32>,
    pub linux_pid: Option<u32>,
    pub used: Option<bool>,
}

impl IdentityPatch {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A process that exits this soon after its start, with an error, did not start.
pub const EARLY_EXIT_MS: f64 = 5000.0;
/// How often the registry is read for running sessions.
pub const POLL_MS: u64 = 1000;
/// How long looking for a transcript in a distribution may take; without an answer, a resume is
/// tried.
pub const TRANSCRIPT_TIMEOUT_MS: u64 = 5000;

pub fn is_running(status: SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Starting | SessionStatus::Idle | SessionStatus::Busy | SessionStatus::Waiting
    )
}

/// How to start a session that should resume. Claude Code writes a transcript only from the
/// first message on; without one there is nothing to resume: an unused session starts fresh
/// under the same id, a used one has lost its history (`None`).
pub fn launch_mode(used: bool, requested: LaunchMode, transcript: bool) -> Option<LaunchMode> {
    if requested != LaunchMode::Resume || transcript {
        return Some(requested);
    }
    if used { None } else { Some(LaunchMode::New) }
}

/// What a process's exit means for its session: ended, or failed to start.
///
/// `stopping`: reframe is ending it. Ended by reframe (e.g. quitting) while still starting up is
/// no failure to start.
pub fn exit_outcome(
    stopping: bool,
    ran_for_ms: f64,
    exit_code: i32,
    mode: LaunchMode,
) -> (SessionStatus, Option<SessionError>) {
    let early = !stopping && ran_for_ms < EARLY_EXIT_MS && exit_code != 0;
    if !early {
        return (SessionStatus::Ended, None);
    }
    let error = if mode == LaunchMode::Resume {
        SessionError::ResumeFailed
    } else {
        SessionError::SpawnFailed
    };
    (SessionStatus::Error, Some(error))
}

/// Whether a registry entry under the session's process id was left behind by another process:
/// a claude killed before it could remove it, whose id Windows gave to this one. Its session is
/// not this one's.
///
/// Not asked for a session in WSL (entries are found by their session id there, and the
/// distribution's clock may differ) nor for one run through cmd.exe (the process id is cmd's).
pub fn is_stale_entry(entry: &RegistryEntry, process_start: Option<&str>, started_at_ms: f64) -> bool {
    match (entry.proc_start.as_deref(), process_start) {
        (Some(recorded), Some(actual)) => recorded != actual,
        _ => entry.started_at.is_some_and(|started| started < started_at_ms - 2000.0),
    }
}

/// What a hook changes about a running session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HookEffects {
    pub identity: IdentityPatch,
    pub status: Option<SessionStatus>,
    pub notify: Option<NotifyKind>,
}

/// `event` is the hook's name (`UserPromptSubmit`, `Stop`, `Notification`); the rest comes from
/// its payload, empty values counting as absent.
pub fn hook_effects(
    event: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    notification_type: Option<&str>,
) -> HookEffects {
    let filled = |value: Option<&str>| value.filter(|value| !value.is_empty()).map(str::to_owned);
    let mut effects = HookEffects {
        identity: IdentityPatch {
            claude_session_id: filled(session_id),
            cwd: filled(cwd),
            used: (event == "UserPromptSubmit").then_some(true),
            ..IdentityPatch::default()
        },
        ..HookEffects::default()
    };
    match event {
        "UserPromptSubmit" => effects.status = Some(SessionStatus::Busy),
        "Stop" => {
            effects.status = Some(SessionStatus::Idle);
            effects.notify = Some(NotifyKind::Done);
        }
        // idle_prompt repeats "waiting for input" a minute after Stop; that was already announced.
        "Notification" if notification_type != Some("idle_prompt") => {
            effects.status = Some(SessionStatus::Waiting);
            effects.notify = Some(NotifyKind::Waiting);
        }
        _ => {}
    }
    effects
}

/// What the manager remembers of a session's registry entry between reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegistryMemory {
    pub name: Option<String>,
    pub status: Option<RegistryStatus>,
}

/// Where a session runs, as far as following its registry entry goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryPlace<'a> {
    /// Claude is the pty's process.
    Direct,
    /// Run through cmd.exe: the pty's process is cmd's, the entry is claude's own.
    ThroughCmd,
    /// In a WSL distribution, reached through `share`.
    Wsl { distro: &'a str, share: &'a str },
}

/// What a registry entry changes about a running session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegistryEffects {
    pub identity: IdentityPatch,
    /// Claude's name for the session changed (the tile's label may follow).
    pub name_changed: bool,
    pub status: Option<SessionStatus>,
    pub notify: Option<NotifyKind>,
}

fn status_of(status: RegistryStatus) -> SessionStatus {
    match status {
        RegistryStatus::Idle => SessionStatus::Idle,
        RegistryStatus::Busy => SessionStatus::Busy,
        RegistryStatus::Waiting => SessionStatus::Waiting,
    }
}

/// Follows a session's registry entry. `memory` is updated to what was seen.
pub fn follow_registry(
    entry: &RegistryEntry,
    memory: &mut RegistryMemory,
    current: SessionStatus,
    place: RegistryPlace,
) -> RegistryEffects {
    // In WSL the folder is a Linux path; reframe keeps it as Windows reaches it.
    let cwd = match (entry.cwd.as_deref().filter(|cwd| !cwd.is_empty()), place) {
        (Some(cwd), RegistryPlace::Wsl { distro, share }) => Some(wsl_windows_path(distro, cwd, share)),
        (cwd, _) => cwd.map(str::to_owned),
    };
    let mut effects = RegistryEffects {
        identity: IdentityPatch {
            claude_session_id: Some(entry.session_id.clone()),
            cwd,
            used: (entry.status == Some(RegistryStatus::Busy)).then_some(true),
            linux_pid: matches!(place, RegistryPlace::Wsl { .. }).then_some(entry.pid),
            // Through cmd.exe the pty's process is cmd's; the session's is claude's own, for
            // cleaning up after a crash.
            pid: (place == RegistryPlace::ThroughCmd).then_some(entry.pid),
        },
        ..RegistryEffects::default()
    };
    if entry.name != memory.name {
        memory.name = entry.name.clone();
        effects.name_changed = true;
    }
    // Follow the registry only when it changes, so it does not undo a fresher hook-based status.
    match entry.status {
        Some(status) if Some(status) != memory.status => {
            let before = memory.status.replace(status);
            effects.status = Some(status_of(status));
            // Without hooks (WSL), the registry is also what tells that Claude is done or needs
            // the user.
            if matches!(place, RegistryPlace::Wsl { .. }) {
                if status == RegistryStatus::Waiting {
                    effects.notify = Some(NotifyKind::Waiting);
                } else if status == RegistryStatus::Idle && before == Some(RegistryStatus::Busy) {
                    effects.notify = Some(NotifyKind::Done);
                }
            }
        }
        status if current == SessionStatus::Starting => {
            effects.status = Some(status.map_or(SessionStatus::Idle, status_of));
        }
        _ => {}
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: Option<RegistryStatus>) -> RegistryEntry {
        RegistryEntry {
            pid: 500,
            session_id: "c-1".into(),
            cwd: Some("/home/me/shop".into()),
            name: Some("shop".into()),
            name_source: None,
            status,
            started_at: None,
            proc_start: None,
        }
    }

    #[test]
    fn a_resume_without_a_transcript_starts_an_unused_session_fresh_and_loses_a_used_one() {
        assert_eq!(launch_mode(true, LaunchMode::Resume, true), Some(LaunchMode::Resume));
        assert_eq!(launch_mode(false, LaunchMode::Resume, false), Some(LaunchMode::New));
        assert_eq!(launch_mode(true, LaunchMode::Resume, false), None);
        assert_eq!(launch_mode(true, LaunchMode::New, false), Some(LaunchMode::New));
    }

    #[test]
    fn an_early_exit_with_an_error_is_a_failed_start_unless_reframe_ended_it() {
        use SessionStatus::{Ended, Error};
        assert_eq!(
            exit_outcome(false, 1200.0, 1, LaunchMode::Resume),
            (Error, Some(SessionError::ResumeFailed))
        );
        assert_eq!(
            exit_outcome(false, 1200.0, 1, LaunchMode::New),
            (Error, Some(SessionError::SpawnFailed))
        );
        assert_eq!(exit_outcome(true, 1200.0, 1, LaunchMode::New), (Ended, None));
        assert_eq!(exit_outcome(false, 1200.0, 0, LaunchMode::New), (Ended, None));
        assert_eq!(exit_outcome(false, 5000.0, 1, LaunchMode::New), (Ended, None));
    }

    #[test]
    fn an_entry_of_another_process_is_told_by_its_start_or_else_by_its_age() {
        let mut left_behind = entry(None);
        left_behind.proc_start = Some("100".into());
        assert!(is_stale_entry(&left_behind, Some("200"), 10_000.0));
        assert!(!is_stale_entry(&left_behind, Some("100"), 10_000.0));
        // Where the system does not tell: an entry from well before this process started.
        left_behind.started_at = Some(7_000.0);
        assert!(is_stale_entry(&left_behind, None, 10_000.0));
        left_behind.started_at = Some(8_500.0);
        assert!(!is_stale_entry(&left_behind, None, 10_000.0));
        assert!(!is_stale_entry(&entry(None), None, 10_000.0));
    }

    #[test]
    fn hooks_set_the_status_and_announce_done_and_waiting() {
        let prompt = hook_effects("UserPromptSubmit", Some("c-2"), Some("C:\\w"), None);
        assert_eq!(prompt.status, Some(SessionStatus::Busy));
        assert_eq!(prompt.notify, None);
        assert_eq!(
            prompt.identity,
            IdentityPatch {
                claude_session_id: Some("c-2".into()),
                cwd: Some("C:\\w".into()),
                used: Some(true),
                ..IdentityPatch::default()
            }
        );

        let stop = hook_effects("Stop", None, None, None);
        assert_eq!(
            (stop.status, stop.notify),
            (Some(SessionStatus::Idle), Some(NotifyKind::Done))
        );
        assert!(stop.identity.is_empty());

        let asking = hook_effects("Notification", None, None, Some("permission_prompt"));
        assert_eq!(
            (asking.status, asking.notify),
            (Some(SessionStatus::Waiting), Some(NotifyKind::Waiting))
        );
    }

    #[test]
    fn the_idle_prompt_a_minute_after_stop_is_not_announced_again() {
        let repeated = hook_effects("Notification", Some("c-1"), None, Some("idle_prompt"));
        assert_eq!((repeated.status, repeated.notify), (None, None));
        assert_eq!(repeated.identity.claude_session_id.as_deref(), Some("c-1"));
    }

    #[test]
    fn the_registry_is_followed_only_when_it_changes() {
        let mut memory = RegistryMemory::default();
        let first = follow_registry(
            &entry(Some(RegistryStatus::Busy)),
            &mut memory,
            SessionStatus::Starting,
            RegistryPlace::Direct,
        );
        assert_eq!(first.status, Some(SessionStatus::Busy));
        assert!(first.name_changed);
        assert_eq!(first.identity.used, Some(true));
        assert_eq!(first.identity.cwd.as_deref(), Some("/home/me/shop"));
        assert_eq!((first.identity.pid, first.identity.linux_pid), (None, None));

        // A hook said idle since; the registry still says busy and must not undo that.
        let again = follow_registry(
            &entry(Some(RegistryStatus::Busy)),
            &mut memory,
            SessionStatus::Idle,
            RegistryPlace::Direct,
        );
        assert_eq!(again.status, None);
        assert!(!again.name_changed);
    }

    #[test]
    fn a_starting_session_with_an_entry_is_idle_until_the_entry_says_otherwise() {
        let mut memory = RegistryMemory::default();
        let effects = follow_registry(
            &entry(None),
            &mut memory,
            SessionStatus::Starting,
            RegistryPlace::Direct,
        );
        assert_eq!(effects.status, Some(SessionStatus::Idle));
        let later = follow_registry(&entry(None), &mut memory, SessionStatus::Idle, RegistryPlace::Direct);
        assert_eq!(later.status, None);
    }

    #[test]
    fn in_wsl_the_registry_also_announces_and_gives_the_linux_pid_and_a_windows_folder() {
        let place = RegistryPlace::Wsl {
            distro: "Ubuntu",
            share: "\\\\wsl.localhost",
        };
        let mut memory = RegistryMemory::default();
        let busy = follow_registry(
            &entry(Some(RegistryStatus::Busy)),
            &mut memory,
            SessionStatus::Idle,
            place,
        );
        assert_eq!(busy.notify, None);
        assert_eq!(busy.identity.linux_pid, Some(500));
        assert_eq!(
            busy.identity.cwd.as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\shop")
        );

        let done = follow_registry(
            &entry(Some(RegistryStatus::Idle)),
            &mut memory,
            SessionStatus::Busy,
            place,
        );
        assert_eq!(done.notify, Some(NotifyKind::Done));
        let asking = follow_registry(
            &entry(Some(RegistryStatus::Waiting)),
            &mut memory,
            SessionStatus::Idle,
            place,
        );
        assert_eq!(asking.notify, Some(NotifyKind::Waiting));
        // Idle without having been busy is no "done".
        let idle = follow_registry(
            &entry(Some(RegistryStatus::Idle)),
            &mut memory,
            SessionStatus::Waiting,
            place,
        );
        assert_eq!(idle.notify, None);
    }

    #[test]
    fn through_cmd_the_entry_gives_claudes_own_pid() {
        let mut memory = RegistryMemory::default();
        let effects = follow_registry(
            &entry(None),
            &mut memory,
            SessionStatus::Idle,
            RegistryPlace::ThroughCmd,
        );
        assert_eq!(effects.identity.pid, Some(500));
    }
}
