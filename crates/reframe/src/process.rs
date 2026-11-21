//! Process helpers. Killing the whole tree matters: claude starts children (MCP servers, tools)
//! and a dying pseudo console does not reliably take them down on Windows.
//!
//! Ported from the Electron version's `src/main/process.ts` and the process functions of
//! `src/main/win32.ts`.

#[cfg(target_os = "linux")]
pub use self::linux::*;
#[cfg(windows)]
pub use self::win32::*;

/// Keeps a helper process from opening a console window of its own (on Windows; elsewhere it
/// opens none anyway).
pub trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        }
        #[cfg(not(windows))]
        {
            self
        }
    }
}

#[cfg(windows)]
mod win32 {
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

    /// When a process started, as the system tells it: its creation time (a FILETIME as a
    /// decimal string), which Claude Code's registry has too (`procStart`). `None` where that
    /// cannot be told.
    pub fn process_start(pid: u32) -> Option<String> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
            let told = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
            CloseHandle(process);
            told.then(|| ((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)).to_string())
        }
    }

    /// Ends a process and everything it started. Blocks until taskkill is done (seconds at
    /// most): call it off the UI thread.
    pub fn kill_tree(pid: u32) {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    /// The process that started a process, as Windows remembers it (it may be gone by now).
    pub fn parent_of(pid: u32) -> Option<u32> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
        };
        // SAFETY: the snapshot is walked with an entry of the size it says, and closed.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut parent = None;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID == pid {
                    parent = Some(entry.th32ParentProcessID);
                    break;
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            parent
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    /// The fields of `/proc/<pid>/stat` after the program's name (which may hold spaces and
    /// brackets): the state is the first, the parent the second, the start time the twentieth.
    fn stat_fields(pid: u32) -> Option<Vec<String>> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after_name = &stat[stat.rfind(')')? + 1..];
        Some(after_name.split_whitespace().map(str::to_owned).collect())
    }

    /// Whether a process with this id runs (one that has exited but was not waited for yet does
    /// not).
    pub fn is_alive(pid: u32) -> bool {
        let Ok(id) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if id <= 0 {
            return false;
        }
        // SAFETY: signal 0 only asks whether the process is there.
        let there =
            unsafe { libc::kill(id, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        there && stat_fields(pid).is_none_or(|fields| fields.first().map(String::as_str) != Some("Z"))
    }

    /// When a process started, as the system tells it: clock ticks after boot
    /// (`/proc/<pid>/stat`, field 22), which Claude Code's registry has too (`procStart`). `None`
    /// where that cannot be told.
    pub fn process_start(pid: u32) -> Option<String> {
        stat_fields(pid)?.get(19).cloned()
    }

    /// The process that started a process.
    pub fn parent_of(pid: u32) -> Option<u32> {
        stat_fields(pid)?.get(1)?.parse().ok()
    }

    /// Every process `pid` started, and what those started, as `/proc` tells it now.
    fn descendants(pid: u32) -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let parents: Vec<(u32, u32)> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|child| Some((child, parent_of(child)?)))
            .collect();
        let mut found = vec![pid];
        let mut next = 0;
        while next < found.len() {
            let parent = found[next];
            found.extend(parents.iter().filter(|(_, of)| *of == parent).map(|(child, _)| *child));
            next += 1;
        }
        found.remove(0);
        found
    }

    /// Ends a process and everything it started: its process group (a session's claude leads
    /// one, as node-pty's did in 0.x) and what left it. Does not block.
    pub fn kill_tree(pid: u32) {
        let Ok(id) = libc::pid_t::try_from(pid) else {
            return;
        };
        if id <= 1 {
            return;
        }
        let children = descendants(pid);
        // SAFETY: plain signals to a process group and to processes by id.
        unsafe {
            if libc::getpgid(id) == id {
                libc::kill(-id, libc::SIGKILL);
            }
            libc::kill(id, libc::SIGKILL);
            for child in children {
                if let Ok(child) = libc::pid_t::try_from(child) {
                    libc::kill(child, libc::SIGKILL);
                }
            }
        }
    }

    /// Ctrl+C as a terminal sends it: SIGINT.
    pub fn interrupt(pid: u32) -> Result<(), String> {
        let id = libc::pid_t::try_from(pid).map_err(|error| error.to_string())?;
        // SAFETY: a plain signal to a process by id.
        if unsafe { libc::kill(id, libc::SIGINT) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }

    #[cfg(test)]
    mod tests {
        use std::process::Command;

        use super::*;

        #[test]
        fn this_process_is_alive_and_has_a_start_and_a_parent() {
            let me = std::process::id();
            assert!(is_alive(me));
            assert!(process_start(me).is_some_and(|start| start.parse::<u64>().is_ok()));
            assert_eq!(parent_of(me), Some(std::os::unix::process::parent_id()));
            assert!(!is_alive(0));
        }

        #[test]
        fn a_tree_is_killed_with_its_children() {
            let mut child = Command::new("sh").args(["-c", "sleep 30 & wait"]).spawn().unwrap();
            let pid = child.id();
            std::thread::sleep(std::time::Duration::from_millis(200));
            let grandchildren = descendants(pid);
            assert!(!grandchildren.is_empty());
            kill_tree(pid);
            let _ = child.wait();
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert!(grandchildren.iter().all(|pid| !is_alive(*pid)));
        }
    }
}
