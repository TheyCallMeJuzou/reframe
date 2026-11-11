//! When a session was last used, for the session list: a prompt sent, or Claude done or asking
//! something. Starting or resuming one (as every start of reframe does) is not using it.
//!
//! Ported from the Electron version's `src/shared/last-used.ts`.

/// What a session is doing. Defined in the Electron version's `src/shared/ipc.ts`; it lives here
/// until the module that reports a session's status is ported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionStatus {
    Starting,
    Idle,
    Busy,
    Waiting,
    Ended,
    Error,
}

/// Whether a session's status going from `before` to `after` is a use of it.
pub fn counts_as_use(before: SessionStatus, after: SessionStatus) -> bool {
    if before == after {
        return false;
    }
    // A prompt (busy), Claude done with it (busy → anything), or Claude asking the user (waiting).
    after == SessionStatus::Busy || after == SessionStatus::Waiting || before == SessionStatus::Busy
}

#[cfg(test)]
mod tests {
    use super::SessionStatus::*;
    use super::*;

    #[test]
    fn counts_a_prompt_claude_done_and_claude_asking() {
        assert!(counts_as_use(Idle, Busy));
        assert!(counts_as_use(Busy, Idle));
        assert!(counts_as_use(Busy, Waiting));
        assert!(counts_as_use(Idle, Waiting));
        assert!(counts_as_use(Waiting, Busy));
    }

    #[test]
    fn does_not_count_starting_resuming_or_ending_one() {
        assert!(!counts_as_use(Starting, Idle));
        assert!(!counts_as_use(Idle, Ended));
        assert!(!counts_as_use(Idle, Error));
        assert!(!counts_as_use(Waiting, Idle));
        assert!(!counts_as_use(Busy, Busy));
    }
}
