//! Pure decisions for taking over a terminal window that was dragged onto reframe: which programs
//! host terminal windows, which claudes live in a window, whether only shells are left in a tab,
//! and which session the window shows.
//!
//! Ported from the Electron version's `src/main/foreign-match.ts` and the types of
//! `src/consolehelper/protocol.ts`, with their tests.

use serde::{Deserialize, Serialize};

/// For a process: its console window and that window's owner (the terminal window hosting the
/// tab). Window handles as numbers; `None` when the process has no console or attaching failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOwner {
    pub pid: u32,
    pub console_window: Option<isize>,
    pub owner: Option<isize>,
    pub root_owner: Option<isize>,
}

/// A process attached to a console.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachedProcess {
    pub pid: u32,
    /// Executable file name, e.g. "pwsh.exe"; `None` when it could not be read.
    pub name: Option<String>,
}

/// Programs that host terminal windows; dragging any other window is ignored right away.
const TERMINAL_HOSTS: [&str; 13] = [
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe",
    "conemu64.exe",
    "conemu.exe",
    "tabby.exe",
    "hyper.exe",
];

/// Shells (and console hosts) that may be left in a terminal tab once claude has gone.
const SHELLS: [&str; 11] = [
    "pwsh.exe",
    "powershell.exe",
    "cmd.exe",
    "bash.exe",
    "sh.exe",
    "zsh.exe",
    "fish.exe",
    "nu.exe",
    "wsl.exe",
    "conhost.exe",
    "openconsole.exe",
];

pub fn is_terminal_host(image_name: Option<&str>) -> bool {
    image_name.is_some_and(|name| TERMINAL_HOSTS.contains(&name.to_lowercase().as_str()))
}

pub fn is_shell(image_name: Option<&str>) -> bool {
    image_name.is_some_and(|name| SHELLS.contains(&name.to_lowercase().as_str()))
}

/// Pids whose console lives in window `hwnd`. Windows Terminal makes the window that hosts a tab
/// the owner of the tab's (hidden) pseudo-console window; a classic console window is the console
/// window itself.
pub fn pids_in_window(hwnd: isize, owners: &[ConsoleOwner]) -> Vec<u32> {
    owners
        .iter()
        .filter(|owner| [owner.console_window, owner.owner, owner.root_owner].contains(&Some(hwnd)))
        .map(|owner| owner.pid)
        .collect()
}

/// Pids of the claudes in the dragged window `hwnd`. A tab torn out of a Windows Terminal window
/// lands in a new window, while its console may still name the old window for a moment, which is
/// gone if that was its last tab: a live claude whose window is gone has moved into the window
/// being dragged.
pub fn pids_for_drop(hwnd: isize, owners: &[ConsoleOwner], is_window: &dyn Fn(isize) -> bool) -> Vec<u32> {
    let direct = pids_in_window(hwnd, owners);
    if !direct.is_empty() {
        return direct;
    }
    owners
        .iter()
        .filter(|owner| {
            owner
                .owner
                .or(owner.console_window)
                .is_some_and(|window| !is_window(window))
        })
        .map(|owner| owner.pid)
        .collect()
}

/// Whether nothing but shells runs in a console (an empty list means the console is already
/// gone).
pub fn only_shells(processes: &[AttachedProcess]) -> bool {
    processes.iter().all(|process| is_shell(process.name.as_deref()))
}

/// The one of `names` the window shows right now, judged by its title (Windows Terminal shows the
/// active tab's): the longest name the title holds.
pub fn active_by_title(title: &str, names: &[Option<&str>]) -> Option<usize> {
    names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            name.filter(|name| !name.is_empty() && title.contains(name))
                .map(|name| (index, name))
        })
        .max_by_key(|(_, name)| name.len())
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(pid: u32, console: Option<isize>, owner: Option<isize>, root: Option<isize>) -> ConsoleOwner {
        ConsoleOwner {
            pid,
            console_window: console,
            owner,
            root_owner: root,
        }
    }

    #[test]
    fn recognizes_terminal_hosts_regardless_of_case() {
        assert!(is_terminal_host(Some("WindowsTerminal.exe")));
        assert!(is_terminal_host(Some("windowsterminal.EXE")));
        assert!(!is_terminal_host(Some("chrome.exe")));
        assert!(!is_terminal_host(None));
    }

    #[test]
    fn maps_consoles_to_the_window_hosting_them() {
        let owners = [
            owner(1, Some(100), Some(500), Some(500)), // tab in Windows Terminal window 500
            owner(2, Some(200), Some(600), Some(600)), // tab in another window
            owner(3, Some(700), None, Some(700)),      // classic console window 700
            owner(4, None, None, None),                // could not attach
        ];
        assert_eq!(pids_in_window(500, &owners), [1]);
        assert_eq!(pids_in_window(700, &owners), [3]);
        assert!(pids_in_window(999, &owners).is_empty());
    }

    #[test]
    fn finds_a_torn_out_tab_by_the_window_its_console_still_names_which_is_gone() {
        let owners = [
            owner(1, Some(100), Some(500), Some(500)), // tab in a window that still exists
            owner(2, Some(200), Some(600), Some(600)), // tab whose window just closed
        ];
        let exists = |hwnd: isize| hwnd != 600;
        assert_eq!(pids_for_drop(900, &owners, &exists), [2]);
        // A direct match wins.
        assert_eq!(pids_for_drop(500, &owners, &exists), [1]);
        assert!(pids_for_drop(900, &owners, &|_| true).is_empty());
    }

    #[test]
    fn closes_only_consoles_with_nothing_but_shells_left() {
        let process = |pid: u32, name: Option<&str>| AttachedProcess {
            pid,
            name: name.map(str::to_owned),
        };
        assert!(only_shells(&[
            process(1, Some("pwsh.exe")),
            process(2, Some("OpenConsole.exe"))
        ]));
        assert!(!only_shells(&[
            process(1, Some("pwsh.exe")),
            process(2, Some("node.exe"))
        ]));
        assert!(!only_shells(&[process(1, None)]));
        assert!(only_shells(&[]));
    }

    #[test]
    fn picks_the_session_whose_name_the_window_title_shows() {
        let names = [Some("api"), Some("api-docs"), None];
        assert_eq!(active_by_title("✳ api-docs", &names), Some(1));
        assert_eq!(active_by_title("✳ Claude Code", &names), None);
    }

    #[test]
    fn owners_travel_as_json_between_reframe_and_its_helper() {
        let owners = vec![owner(7, Some(1), None, Some(2))];
        let text = serde_json::to_string(&owners).unwrap();
        assert_eq!(text, r#"[{"pid":7,"consoleWindow":1,"owner":null,"rootOwner":2}]"#);
        assert_eq!(serde_json::from_str::<Vec<ConsoleOwner>>(&text).unwrap(), owners);
    }
}
