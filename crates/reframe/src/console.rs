//! Typing into another process's console, as if keys were pressed there. A process can be
//! attached to one console only, and reframe must never switch its own, so this runs in a
//! short-lived copy of reframe started for it (`--console-helper`).
//!
//! Ported from the Electron version's `src/consolehelper/index.ts` (its `interrupt` request; the
//! others follow with terminal windows dragged onto reframe) and `src/main/takeover.ts`.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0,
    LEFT_CTRL_PRESSED, WriteConsoleInputW,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::process::{is_alive, kill_tree};

/// The argument that makes reframe a console helper.
pub const HELPER_ARG: &str = "--console-helper";

/// Ctrl+C as a console key event.
fn ctrl_c(down: bool) -> INPUT_RECORD {
    INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: i32::from(down),
                wRepeatCount: 1,
                wVirtualKeyCode: 0x43,
                wVirtualScanCode: 0x2e,
                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 3 },
                dwControlKeyState: LEFT_CTRL_PRESSED,
            },
        },
    }
}

/// Types Ctrl+C `times` times into the console of `pid`, with a pause between them.
fn interrupt(pid: u32, times: u32, pause: Duration) -> Result<(), String> {
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return Err(format!("AttachConsole failed ({})", GetLastError()));
        }
        let name: Vec<u16> = "CONIN$\0".encode_utf16().collect();
        let input = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if input.is_null() || input == INVALID_HANDLE_VALUE {
            let error = format!("CONIN$ not available ({})", GetLastError());
            FreeConsole();
            return Err(error);
        }
        let mut result = Ok(());
        for time in 0..times {
            if time > 0 {
                std::thread::sleep(pause);
            }
            let keys = [ctrl_c(true), ctrl_c(false)];
            let mut written = 0;
            if WriteConsoleInputW(input, keys.as_ptr(), 2, &mut written) == 0 {
                result = Err(format!("WriteConsoleInput failed ({})", GetLastError()));
                break;
            }
        }
        CloseHandle(input);
        FreeConsole();
        result
    }
}

/// Started as a console helper: does what was asked and gives the exit code to leave with.
/// `None` when this is not a console helper.
pub fn run_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(HELPER_ARG) {
        return None;
    }
    let result = match (
        args.get(1).map(String::as_str),
        args.get(2).and_then(|pid| pid.parse().ok()),
    ) {
        (Some("interrupt"), Some(pid)) => interrupt(pid, 2, Duration::from_millis(700)),
        _ => Err(format!("unknown request: {:?}", &args[1..])),
    };
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

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

/// Ends a claude that runs outside reframe the way its user would: Ctrl+C twice in its terminal.
/// Only when that does not work is the process tree killed. Used before reframe continues the
/// session itself, so the same conversation never runs in two places. Blocks for seconds: call it
/// off the UI thread. `warn` is told what went wrong on the way.
pub fn stop_external_claude(pid: u32, warn: &dyn Fn(&str, String)) -> StopResult {
    if !is_alive(pid) {
        return StopResult::Exited;
    }
    let typed = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|exe| {
            Command::new(exe)
                .args([HELPER_ARG, "interrupt", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|error| error.to_string())
        })
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
            }
        });
    if let Err(error) = typed {
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
