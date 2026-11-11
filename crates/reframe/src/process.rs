//! Process helpers. Killing the whole tree matters: claude starts children (MCP servers, tools)
//! and a dying pseudo console does not reliably take them down on Windows.
//!
//! Ported from the Electron version's `src/main/process.ts` and the process functions of
//! `src/main/win32.ts`.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, FILETIME, GetLastError, STILL_ACTIVE};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Whether a process with this id runs.
pub fn is_alive(pid: u32) -> bool {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            // One that may not be looked at is there all the same.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0;
        let told = GetExitCodeProcess(process, &mut code) != 0;
        CloseHandle(process);
        // A process that has exited stays openable while someone still holds a handle to it.
        !told || code == STILL_ACTIVE as u32
    }
}

/// When a process started, as the system tells it: its creation time (a FILETIME as a decimal
/// string), which Claude Code's registry has too (`procStart`). `None` where that cannot be told.
pub fn process_start(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
        let told = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
        CloseHandle(process);
        told.then(|| ((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)).to_string())
    }
}

/// Ends a process and everything it started. Blocks until taskkill is done (seconds at most):
/// call it off the UI thread.
pub fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}
