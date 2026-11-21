//! Ending a claude that runs outside reframe, before reframe continues its session, so the same
//! conversation never runs in two places.
//!
//! Ported from the Electron version's `src/main/takeover.ts`.

use std::time::{Duration, Instant};

use crate::process::{is_alive, kill_tree};

/// How a claude outside reframe was ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopResult {
    Exited,
    Killed,
    Failed,
}

fn exits_within(pid: u32, time: Duration) -> bool {
    let end = Instant::now() + time;
    while Instant::now() < end {
        if !is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !is_alive(pid)
}

/// Ctrl+C twice in the claude's terminal, as its user would. On Windows it is typed into its
/// console; elsewhere it is the signal a terminal sends for it, 700 ms apart (as 0.x did).
fn interrupt(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::console::interrupt(pid)
    }
    #[cfg(not(windows))]
    {
        for time in 0..2 {
            if time > 0 {
                std::thread::sleep(Duration::from_millis(700));
            }
            if !is_alive(pid) {
                break;
            }
            crate::process::interrupt(pid)?;
        }
        Ok(())
    }
}

/// Ends a claude that runs outside reframe the way its user would: Ctrl+C twice in its terminal.
/// Only when that does not work is the process tree killed. Blocks for seconds: call it off the
/// UI thread. `warn` is told what went wrong on the way.
pub fn stop_external_claude(pid: u32, warn: &dyn Fn(&str, String)) -> StopResult {
    if !is_alive(pid) {
        return StopResult::Exited;
    }
    if let Err(error) = interrupt(pid) {
        warn("could not type Ctrl+C into the other terminal", error);
    }
    if exits_within(pid, Duration::from_millis(6000)) {
        return StopResult::Exited;
    }
    warn(
        "claude in the other terminal did not exit on Ctrl+C, killing it",
        pid.to_string(),
    );
    kill_tree(pid);
    if exits_within(pid, Duration::from_millis(3000)) {
        StopResult::Killed
    } else {
        StopResult::Failed
    }
}
