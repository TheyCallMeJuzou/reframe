//! What reframe asks WSL distributions, and how it reads the answers: the list of distributions,
//! the probe for claude (through the user's login shell), and a distribution's `/proc` (whether a
//! process is a claude, a shell, which terminal it runs in). Running `wsl.exe` and reading the
//! share is the app's; this is what it runs and how it reads what comes back.
//!
//! Ported from the pure part of the Electron version's `src/main/wsl/distros.ts`, with its tests.

/// Between this and the end comes the probe's answer, whatever the shell's startup files print.
pub const MARKER: &str = "@@REFRAME@@";

/// Asks the user's login shell (interactive, so .bashrc runs too) for what reframe needs, between
/// markers so that whatever the shell's startup files print does not get in the way; falls back
/// to plain sh when that shell fails.
pub const PROBE_SCRIPT: &str = concat!(
    "s=\"${SHELL:-/bin/sh}\"\n",
    "ask='printf \"\\n@@REFRAME@@\\n%s\\n%s\\n%s\\n%s\\n\" \"$PATH\" \"$(command -v claude)\" \"$HOME\" \"${CLAUDE_CONFIG_DIR:-}\"'\n",
    "out=$(\"$s\" -lic \"$ask\" 2>/dev/null </dev/null)\n",
    "case \"$out\" in *@@REFRAME@@*) ;; *) out=$(sh -c \"$ask\") ;; esac\n",
    "printf '%s\\n' \"$out\" | sed -n '/^@@REFRAME@@$/,$p'\n",
    "printf 'MOUNT=%s\\n' \"$(wslpath -u 'C:\\' 2>/dev/null)\"",
);

/// What it takes to run claude in a distribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslInfo {
    pub distro: String,
    /// claude in the distribution itself; `None` when there is none (one on a Windows drive does
    /// not count).
    pub claude: Option<String>,
    /// PATH of the user's login shell, which claude (and node, for an npm install of it) needs.
    pub path: String,
    pub home: String,
    /// Claude Code's config folder, as a Linux path.
    pub claude_home: String,
    /// Where the Windows drives are, e.g. `/mnt/`.
    pub mount_root: String,
}

/// wsl.exe writes UTF-16 unless WSL_UTF8 is set, which older versions ignore.
pub fn decode_wsl_output(output: &[u8]) -> String {
    let utf16 = output.len() >= 2 && output[1] == 0 && output[0] != 0;
    let text = if utf16 {
        let units: Vec<u16> = output
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(output).into_owned()
    };
    text.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(text)
}

/// Distribution names from `wsl.exe -l -q`.
pub fn parse_distro_list(output: &str) -> Vec<String> {
    output
        .split('\n')
        .map(|line| line.replace('\0', "").trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// What the probe found, or `None` when it did not answer.
pub fn parse_probe(distro: &str, output: &str) -> Option<WslInfo> {
    let text = output.replace('\r', "");
    let lines: Vec<&str> = text.split('\n').collect();
    let at = lines.iter().position(|line| *line == MARKER)?;
    let line = |n: usize| lines.get(at + n).copied().unwrap_or_default();
    let (path, claude, home, config_dir) = (line(1), line(2), line(3), line(4));
    if !home.starts_with('/') {
        return None;
    }
    let mount = lines
        .iter()
        .find_map(|line| line.strip_prefix("MOUNT="))
        .filter(|mount| !mount.is_empty())
        .unwrap_or("/mnt/c/");
    // `/mnt/c/` → `/mnt/`: the last part (and the slashes after it) goes.
    let trimmed = mount.trim_end_matches('/');
    let mount_root = match trimmed.rfind('/') {
        Some(at) if at + 1 < trimmed.len() => &trimmed[..=at],
        _ => "",
    };
    let mount_root = if mount_root.is_empty() { "/mnt/" } else { mount_root };
    // The Windows PATH is part of WSL's; a claude found there is the Windows one, which cannot run
    // here.
    let in_distro = claude.starts_with('/') && !claude.starts_with(mount_root);
    Some(WslInfo {
        distro: distro.to_owned(),
        claude: in_distro.then(|| claude.to_owned()),
        path: path.to_owned(),
        home: home.to_owned(),
        claude_home: if config_dir.starts_with('/') {
            config_dir.to_owned()
        } else {
            format!("{}/.claude", home.trim_end_matches('/'))
        },
        mount_root: mount_root.to_owned(),
    })
}

/// Whether a Linux command line (argv, NUL-separated, as /proc/<pid>/cmdline has it) is a Claude
/// Code's: the program `claude` (the native install, or a link to it), or node running its script
/// (an npm, pnpm or yarn install).
pub fn looks_like_claude(cmdline: &str) -> bool {
    cmdline.split('\0').take(2).any(|arg| {
        arg == "claude"
            || arg.ends_with("/claude")
            || arg.contains("/claude-code/")
            || arg.contains("/.local/share/claude/")
    })
}

const SHELLS: [&str; 10] = ["bash", "sh", "dash", "zsh", "fish", "ksh", "mksh", "tcsh", "csh", "nu"];

/// Whether a Linux command line (NUL-separated) is a shell's; a login shell's name starts with
/// "-".
pub fn is_shell_command(cmdline: &str) -> bool {
    let program = cmdline.split('\0').next().unwrap_or_default();
    let name = program.trim_start_matches('-').rsplit('/').next().unwrap_or_default();
    SHELLS.contains(&name)
}

/// The parent's pid in /proc/<pid>/stat, past the program's name (which may hold spaces and
/// parentheses).
pub fn parent_of(stat: &str) -> Option<u32> {
    let end = stat.rfind(')')?;
    let parent: u32 = stat[end + 1..].split_whitespace().nth(1)?.parse().ok()?;
    (parent > 0).then_some(parent)
}

/// The interop socket of a WSL session, from the environment of a process in it
/// (/proc/<pid>/environ).
pub fn interop_of(environ: &str) -> Option<String> {
    environ
        .split('\0')
        .find_map(|variable| variable.strip_prefix("WSL_INTEROP="))
        .filter(|value| value.starts_with('/'))
        .map(str::to_owned)
}

/// The argument that makes reframe print the pid of the process that started it, and leave.
pub const PARENT_PID_ARG: &str = "--print-parent-pid";

/// What runs `program` (a Windows one, reframe's own executable) through the interop socket
/// `interop`, to print the pid of its parent: the program that started that WSL session (the
/// wsl.exe in its terminal tab).
pub fn parent_pid_command(interop: &str, program: &str) -> Vec<String> {
    vec![
        "env".to_owned(),
        format!("WSL_INTEROP={interop}"),
        program.to_owned(),
        PARENT_PID_ARG.to_owned(),
    ]
}

/// The shells between Linux process `pid` and WSL's own process for its terminal, innermost
/// first: the shell claude was typed into, the shell that one was started from, and so on. None
/// when anything else is in between (tmux, say), where an "exit" typed into the terminal could
/// reach another program. `read_proc(pid, file)` reads a file of /proc/<pid>.
pub fn shells_of(pid: u32, read_proc: &dyn Fn(u32, &str) -> Option<String>) -> Vec<u32> {
    let mut shells = Vec::new();
    let mut current = pid;
    for _ in 0..6 {
        let parent = read_proc(current, "stat").as_deref().and_then(parent_of);
        let cmdline = parent
            .and_then(|parent| read_proc(parent, "cmdline"))
            .unwrap_or_default();
        if cmdline.split('\0').next() == Some("/init") {
            return shells;
        }
        let Some(parent) = parent.filter(|_| is_shell_command(&cmdline)) else {
            return Vec::new();
        };
        shells.push(parent);
        current = parent;
    }
    Vec::new()
}

/// Whether Linux process `pid` runs with no program of its own running (a shell at its prompt).
pub fn is_idle(pid: u32, read_proc: &dyn Fn(u32, &str) -> Option<String>) -> bool {
    read_proc(pid, &format!("task/{pid}/children")).is_some_and(|children| children.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_the_list_in_utf16_and_utf8() {
        assert_eq!(
            parse_distro_list(&decode_wsl_output(&utf16("Ubuntu\r\nDebian\r\n"))),
            ["Ubuntu", "Debian"]
        );
        assert_eq!(
            parse_distro_list(&decode_wsl_output("\u{feff}Ubuntu-24.04\r\n\r\n".as_bytes())),
            ["Ubuntu-24.04"]
        );
        assert!(parse_distro_list("").is_empty());
    }

    fn argv(args: &[&str]) -> String {
        args.join("\0")
    }

    #[test]
    fn knows_claude_by_its_command_line() {
        assert!(looks_like_claude(&argv(&["/usr/local/bin/claude", "--resume", "abc"])));
        assert!(looks_like_claude(&argv(&["claude"])));
        assert!(looks_like_claude(&argv(&[
            "/home/ann/.local/share/claude/versions/2.1.309",
            "--session-id",
            "x"
        ])));
        assert!(looks_like_claude(&argv(&[
            "node",
            "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js"
        ])));
        assert!(looks_like_claude(&argv(&[
            "/home/ann/.nvm/versions/node/v22.0.0/bin/node",
            "/home/ann/.local/share/pnpm/global/5/.pnpm/@anthropic-ai+claude-code@2.1.0/node_modules/@anthropic-ai/claude-code/cli.js",
        ])));
        assert!(!looks_like_claude(&argv(&["bash"])));
        assert!(!looks_like_claude(&argv(&["node", "server.js", "--name", "claude"])));
        assert!(!looks_like_claude(&argv(&["vim", "claude-notes.md"])));
        assert!(!looks_like_claude(""));
    }

    fn answer(claude: &str, config_dir: &str) -> String {
        [
            "Welcome to a chatty .bashrc",
            "",
            "@@REFRAME@@",
            "/home/ann/.local/bin:/usr/bin:/mnt/c/Users/Ann/AppData/Roaming/npm",
            claude,
            "/home/ann",
            config_dir,
            "MOUNT=/mnt/c/",
        ]
        .join("\r\n")
    }

    #[test]
    fn the_probe_finds_claude_the_path_the_home_and_the_drives() {
        assert_eq!(
            parse_probe("Ubuntu", &answer("/home/ann/.local/bin/claude", "")),
            Some(WslInfo {
                distro: "Ubuntu".into(),
                claude: Some("/home/ann/.local/bin/claude".into()),
                path: "/home/ann/.local/bin:/usr/bin:/mnt/c/Users/Ann/AppData/Roaming/npm".into(),
                home: "/home/ann".into(),
                claude_home: "/home/ann/.claude".into(),
                mount_root: "/mnt/".into(),
            })
        );
        assert_eq!(
            parse_probe("Ubuntu", &answer("/usr/bin/claude", "/data/claude")).map(|info| info.claude_home),
            Some("/data/claude".into())
        );
    }

    #[test]
    fn the_probe_does_not_count_the_windows_claude_nor_a_missing_one() {
        let windows = parse_probe("Ubuntu", &answer("/mnt/c/Users/Ann/AppData/Roaming/npm/claude", ""));
        assert_eq!(windows.unwrap().claude, None);
        assert_eq!(parse_probe("Ubuntu", &answer("", "")).unwrap().claude, None);
        assert_eq!(parse_probe("Ubuntu", "command not found"), None);
    }

    #[test]
    fn the_probe_script_asks_between_markers() {
        assert!(PROBE_SCRIPT.contains(r#"printf "\n@@REFRAME@@\n%s\n%s\n%s\n%s\n""#));
        assert!(PROBE_SCRIPT.ends_with(r#"printf 'MOUNT=%s\n' "$(wslpath -u 'C:\' 2>/dev/null)""#));
    }

    #[test]
    fn knows_shells_by_their_command_lines() {
        assert!(is_shell_command(&argv(&["-bash"])));
        assert!(is_shell_command(&argv(&["/usr/bin/zsh", "-l"])));
        assert!(is_shell_command(&argv(&["bash", "-lic", "claude"])));
        assert!(is_shell_command(&argv(&["fish"])));
        assert!(!is_shell_command(&argv(&["tmux", "new"])));
        assert!(!is_shell_command(&argv(&["/init"])));
        assert!(!is_shell_command(""));
    }

    #[test]
    fn reads_the_parent_whatever_the_program_is_called() {
        assert_eq!(
            parent_of("29024 (claude) S 341 29024 341 34816 29024 4194304"),
            Some(341)
        );
        assert_eq!(parent_of("77 (my (odd) prog) R 12 77 12 0"), Some(12));
        assert_eq!(parent_of(""), None);
        assert_eq!(parent_of("1 (init) S 0 0"), None);
    }

    #[test]
    fn finds_the_interop_socket() {
        assert_eq!(
            interop_of(&argv(&[
                "HOME=/home/ann",
                "WSL_INTEROP=/run/WSL/340_interop",
                "TERM=xterm"
            ])),
            Some("/run/WSL/340_interop".into())
        );
        assert_eq!(interop_of(&argv(&["HOME=/home/ann"])), None);
        assert_eq!(interop_of(""), None);
    }

    #[test]
    fn runs_reframe_through_the_socket_to_tell_the_terminal() {
        assert_eq!(
            parent_pid_command("/run/WSL/340_interop", "/mnt/c/reframe/reframe.exe"),
            [
                "env",
                "WSL_INTEROP=/run/WSL/340_interop",
                "/mnt/c/reframe/reframe.exe",
                "--print-parent-pid"
            ]
        );
    }

    /// A /proc with these processes: pid → (parent, command line, children).
    fn proc(processes: &[(u32, u32, &str, &[u32])]) -> impl Fn(u32, &str) -> Option<String> {
        let mut files: HashMap<(u32, String), String> = HashMap::new();
        for (pid, parent, cmdline, children) in processes {
            let name = cmdline.split('\0').next().unwrap_or_default();
            files.insert((*pid, "stat".into()), format!("{pid} ({name}) S {parent} {pid}"));
            files.insert((*pid, "cmdline".into()), format!("{cmdline}\0"));
            let children: String = children.iter().map(|child| format!("{child} ")).collect();
            files.insert((*pid, format!("task/{pid}/children")), children);
        }
        move |pid, file| files.get(&(pid, file.to_owned())).cloned()
    }

    #[test]
    fn the_shells_up_to_wsls_own_process_innermost_first() {
        let read = proc(&[
            (10, 2, "/init", &[20]),
            (20, 10, "-bash", &[21]),
            (21, 20, "bash", &[30]),
            (30, 21, "/usr/local/bin/claude", &[]),
        ]);
        assert_eq!(shells_of(30, &read), [21, 20]);
        assert!(!is_idle(21, &read));
        assert!(is_idle(30, &read));
        assert!(!is_idle(99, &read));
    }

    #[test]
    fn no_shells_when_claude_is_the_terminals_program_or_anything_is_in_between() {
        let read = proc(&[
            (10, 2, "/init", &[30, 15]),
            (30, 10, "/usr/local/bin/claude", &[]),
            (15, 10, "tmux", &[20]),
            (20, 15, "-bash", &[31]),
            (31, 20, "/usr/local/bin/claude", &[]),
        ]);
        assert!(shells_of(30, &read).is_empty());
        assert!(shells_of(31, &read).is_empty());
        assert!(shells_of(99, &read).is_empty());
    }
}
