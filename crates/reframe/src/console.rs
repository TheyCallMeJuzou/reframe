//! What needs another process's console: typing into it (Ctrl+C, "exit"), telling which terminal
//! window hosts it, and listing what runs in it. A process can be attached to one console only,
//! and reframe must never switch its own, so this runs in a short-lived copy of reframe started
//! for it (`--console-helper <request> …`), one request per copy; it answers on its output.
//!
//! Ported from the Electron version's `src/consolehelper/index.ts` and `src/main/takeover.ts`.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use reframe_core::foreign_match::{AttachedProcess, ConsoleOwner};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GetConsoleProcessList, GetConsoleWindow, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
    KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, LEFT_CTRL_PRESSED, SHIFT_PRESSED, WriteConsoleInputW,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC, MapVirtualKeyW, VkKeyScanW};
use windows_sys::Win32::UI::WindowsAndMessaging::{GA_ROOTOWNER, GW_OWNER, GetAncestor, GetWindow};

use crate::process::{is_alive, kill_tree};

/// The argument that makes reframe a console helper.
pub const HELPER_ARG: &str = "--console-helper";

/// A key as a console sees it.
#[derive(Clone, Copy)]
struct Key {
    vk: u16,
    scan: u16,
    ch: u16,
    state: u32,
}

const CTRL_C: Key = Key {
    vk: 0x43,
    scan: 0x2e,
    ch: 3,
    state: LEFT_CTRL_PRESSED,
};

/// The key that produces `ch` on the current keyboard layout ("\r" is Enter).
fn key_for(ch: u16) -> Key {
    if ch == u16::from(b'\r') {
        return Key {
            vk: 0x0d,
            scan: 0x1c,
            ch: 13,
            state: 0,
        };
    }
    // SAFETY: plain values in and out.
    let code = unsafe { VkKeyScanW(ch) };
    let vk = (code as u16) & 0xff;
    let shift = (code >> 8) & 1 != 0;
    Key {
        vk,
        // SAFETY: as above.
        scan: unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16,
        ch,
        state: if shift { SHIFT_PRESSED } else { 0 },
    }
}

fn record(key: Key, down: bool) -> INPUT_RECORD {
    INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: i32::from(down),
                wRepeatCount: 1,
                wVirtualKeyCode: key.vk,
                wVirtualScanCode: key.scan,
                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: key.ch },
                dwControlKeyState: key.state,
            },
        },
    }
}

/// Runs `using` while attached to the console of `pid`; always detaches again.
fn with_console<T>(pid: u32, using: impl FnOnce() -> T) -> Result<T, String> {
    // SAFETY: attaching and detaching this process's console.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return Err(format!("AttachConsole({pid}) failed ({})", GetLastError()));
        }
        let value = using();
        FreeConsole();
        Ok(value)
    }
}

/// Types groups of keys into the console of `pid`, as if pressed there, with a pause between
/// groups.
fn type_keys(pid: u32, groups: &[Vec<Key>], pause: Duration) -> Result<(), String> {
    with_console(pid, || {
        // SAFETY: the console's input is opened, written to and closed here.
        unsafe {
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
                return Err(format!("CONIN$ not available ({})", GetLastError()));
            }
            let mut result = Ok(());
            'groups: for (index, keys) in groups.iter().enumerate() {
                if index > 0 {
                    std::thread::sleep(pause);
                }
                for key in keys {
                    let records = [record(*key, true), record(*key, false)];
                    let mut written = 0;
                    if WriteConsoleInputW(input, records.as_ptr(), 2, &mut written) == 0 {
                        result = Err(format!("WriteConsoleInput failed ({})", GetLastError()));
                        break 'groups;
                    }
                }
            }
            CloseHandle(input);
            result
        }
    })?
}

/// For each pid: its console window and that window's owner (the terminal window hosting it).
fn owners(pids: &[u32]) -> Vec<ConsoleOwner> {
    pids.iter()
        .map(|&pid| {
            let found = with_console(pid, || {
                // SAFETY: asking about windows by handle.
                unsafe {
                    let window = GetConsoleWindow();
                    let handle =
                        |window: windows_sys::Win32::Foundation::HWND| (!window.is_null()).then_some(window as isize);
                    let (owner, root) = if window.is_null() {
                        (None, None)
                    } else {
                        (
                            handle(GetWindow(window, GW_OWNER)),
                            handle(GetAncestor(window, GA_ROOTOWNER)),
                        )
                    };
                    ConsoleOwner {
                        pid,
                        console_window: handle(window),
                        owner,
                        root_owner: root,
                    }
                }
            });
            found.unwrap_or(ConsoleOwner {
                pid,
                console_window: None,
                owner: None,
                root_owner: None,
            })
        })
        .collect()
}

/// A process's executable file name, e.g. "pwsh.exe".
pub fn image_name(pid: u32) -> Option<String> {
    // SAFETY: the process is opened, asked and closed here.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let told = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size) != 0;
        CloseHandle(process);
        if !told {
            return None;
        }
        let full = String::from_utf16_lossy(&buffer[..size as usize]);
        Some(full.rsplit('\\').next().unwrap_or(&full).to_owned())
    }
}

/// The processes attached to the console of `pid` (shell, claude, anything else running there),
/// without this helper.
fn console_processes(pid: u32) -> Result<Vec<AttachedProcess>, String> {
    let pids = with_console(pid, || {
        let mut list = [0u32; 256];
        // SAFETY: the list is as long as the count says.
        let count = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) } as usize;
        list[..count.min(list.len())].to_vec()
    })?;
    let own = std::process::id();
    Ok(pids
        .into_iter()
        .filter(|pid| *pid != own)
        .map(|pid| AttachedProcess {
            pid,
            name: image_name(pid),
        })
        .collect())
}

/// Started as a console helper: does what was asked, answers on the output, and gives the exit
/// code to leave with. `None` when this is not a console helper.
pub fn run_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(HELPER_ARG) {
        return None;
    }
    let pid = |at: usize| args.get(at).and_then(|pid| pid.parse::<u32>().ok());
    let result: Result<String, String> = match (args.get(1).map(String::as_str), pid(2)) {
        (Some("interrupt"), Some(pid)) => {
            type_keys(pid, &[vec![CTRL_C], vec![CTRL_C]], Duration::from_millis(700)).map(|()| String::new())
        }
        (Some("type"), Some(pid)) => {
            let text = args.get(3).cloned().unwrap_or_default();
            let keys: Vec<Key> = text.encode_utf16().map(key_for).collect();
            type_keys(pid, &[keys], Duration::ZERO).map(|()| String::new())
        }
        (Some("owners"), _) => {
            let pids: Vec<u32> = args[2..].iter().filter_map(|pid| pid.parse().ok()).collect();
            serde_json::to_string(&owners(&pids)).map_err(|error| error.to_string())
        }
        (Some("console-processes"), Some(pid)) => console_processes(pid)
            .and_then(|processes| serde_json::to_string(&processes).map_err(|error| error.to_string())),
        _ => Err(format!("unknown request: {:?}", &args[1..])),
    };
    Some(match result {
        Ok(answer) => {
            print!("{answer}");
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

/// Runs a console helper with a request; its answer, or what went wrong. Blocks: call it off the
/// UI thread.
fn helper(args: &[String]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(exe)
        .arg(HELPER_ARG)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// For each pid: its console window and the terminal window hosting it.
pub fn console_owners(pids: &[u32]) -> Result<Vec<ConsoleOwner>, String> {
    let mut args = vec!["owners".to_owned()];
    args.extend(pids.iter().map(u32::to_string));
    serde_json::from_str(&helper(&args)?).map_err(|error| error.to_string())
}

/// What runs in the console of `pid`.
pub fn processes_in_console(pid: u32) -> Result<Vec<AttachedProcess>, String> {
    serde_json::from_str(&helper(&["console-processes".to_owned(), pid.to_string()])?)
        .map_err(|error| error.to_string())
}

/// Types text into the console of `pid` ("\r" is Enter), e.g. "exit\r" to leave an idle shell.
pub fn type_into_console(pid: u32, text: &str) -> Result<(), String> {
    helper(&["type".to_owned(), pid.to_string(), text.to_owned()]).map(|_| ())
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
    let typed = helper(&["interrupt".to_owned(), pid.to_string()]);
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
