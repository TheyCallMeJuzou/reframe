//! Finds the claude executable the same way the user's shell would.
//!
//! Ported from the Electron version's `src/main/claude/resolve.ts`. Paths are handled as text,
//! not `std::path`, because Windows paths are resolved here on any platform (as in the tests).

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeBinary {
    pub file: String,
    /// Arguments placed before claude's own (e.g. the script path when running a .js override).
    pub prefix_args: Vec<String>,
    /// Extra environment needed to run `file`.
    pub env: HashMap<String, String>,
    /// Where it was found, for diagnostics.
    pub source: String,
    /// Windows: a launcher (batch file) reframe cannot see through, run by cmd.exe with a command
    /// line of its own (see [`cmd_command_line`]) instead of `prefix_args`.
    pub via_cmd: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    /// macOS and Linux.
    Unix,
}

/// What runs a `.js` stand-in for claude (the tests' fake claude): a node, with the environment
/// it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRunner {
    pub file: String,
    pub env: HashMap<String, String>,
}

pub struct ResolveOptions<'a> {
    pub env: &'a HashMap<String, String>,
    pub platform: Platform,
    /// Explicit path from settings or `REFRAME_CLAUDE_BIN`.
    pub override_path: Option<&'a str>,
    /// Runs `.js` overrides. The Electron version ran them with itself as node; here there is no
    /// node built in, so without a runner a `.js` override is not found.
    pub node_runner: Option<NodeRunner>,
    /// The user's home folder, for the places installers put claude on macOS and Linux.
    pub home: Option<&'a str>,
    pub is_file: &'a dyn Fn(&str) -> bool,
    /// A file's text, or `None` if it cannot be read.
    pub read_file: &'a dyn Fn(&str) -> Option<String>,
}

/// `is_file` for the real file system.
pub fn is_file(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

/// `read_file` for the real file system.
pub fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

pub fn resolve_claude(options: &ResolveOptions) -> Option<ClaudeBinary> {
    let ResolveOptions {
        env,
        platform,
        is_file,
        read_file,
        ..
    } = options;
    let platform = *platform;

    if let Some(chosen) = options.override_path.filter(|path| !path.is_empty()) {
        // One chosen that is not there (any more) is not found, rather than started and failing.
        if !is_file(chosen) {
            return None;
        }
        if is_script(chosen) {
            let runner = options.node_runner.clone()?;
            return Some(ClaudeBinary {
                file: runner.file,
                prefix_args: vec![chosen.to_owned()],
                env: runner.env,
                source: chosen.to_owned(),
                via_cmd: None,
            });
        }
        return Some(wrap(chosen, platform, read_file, is_file, &|| None));
    }

    let path = env
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.as_str());
    let separator = if platform == Platform::Windows { ';' } else { ':' };
    let dirs: Vec<&str> = path
        .unwrap_or("")
        .split(separator)
        .map(|dir| {
            let dir = dir.trim();
            // A folder may be quoted on the PATH.
            if dir.len() >= 2 && dir.starts_with('"') && dir.ends_with('"') {
                &dir[1..dir.len() - 1]
            } else {
                dir
            }
        })
        .filter(|dir| !dir.is_empty())
        .collect();

    if platform == Platform::Windows {
        let extensions = env
            .get("PATHEXT")
            .or_else(|| {
                env.iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("PATHEXT"))
                    .map(|(_, value)| value)
            })
            .map(String::as_str)
            .unwrap_or(".COM;.EXE;.BAT;.CMD");
        let extensions: Vec<&str> = extensions.split(';').filter(|ext| !ext.is_empty()).collect();
        let on_path = |name: &str, only: Option<&[&str]>| -> Option<String> {
            for dir in &dirs {
                for ext in only.unwrap_or(&extensions) {
                    let candidate = win_join(dir, &format!("{name}{}", ext.to_lowercase()));
                    if is_file(&candidate) {
                        return Some(candidate);
                    }
                }
            }
            None
        };
        let claude = on_path("claude", None)?;
        return Some(wrap(&claude, platform, read_file, is_file, &|| {
            on_path("node", Some(&[".exe"]))
        }));
    }

    // GUI apps on macOS/Linux often start with a minimal PATH, so also look where installers put
    // claude.
    let mut places: Vec<String> = dirs.iter().map(|dir| (*dir).to_owned()).collect();
    if let Some(home) = options.home {
        places.push(posix_join(home, ".local/bin"));
        places.push(posix_join(home, ".claude/local"));
        places.push(posix_join(home, ".npm-global/bin"));
    }
    places.push("/opt/homebrew/bin".into());
    places.push("/usr/local/bin".into());
    for dir in places {
        let candidate = posix_join(&dir, "claude");
        if is_file(&candidate) {
            return Some(wrap(&candidate, platform, read_file, is_file, &|| None));
        }
    }
    None
}

/// What runs a claude found on the PATH. On Windows that is often a package manager's batch file
/// (npm, pnpm, yarn) that starts the real program: reframe starts that program itself, so that
/// the pty's process is claude, which is what Claude Code's session registry is keyed by.
fn wrap(
    file: &str,
    platform: Platform,
    read_file: &dyn Fn(&str) -> Option<String>,
    is_file: &dyn Fn(&str) -> bool,
    find_node: &dyn Fn() -> Option<String>,
) -> ClaudeBinary {
    let plain = |file: String, prefix_args: Vec<String>, via_cmd: Option<String>, source: &str| ClaudeBinary {
        file,
        prefix_args,
        env: HashMap::new(),
        source: source.to_owned(),
        via_cmd,
    };
    if platform == Platform::Windows && (ends_with_ignore_case(file, ".cmd") || ends_with_ignore_case(file, ".bat")) {
        // Unreadable: run as it is, below.
        let text = read_file(file).unwrap_or_default();
        let dir = win_dirname(file);
        // The batch file's folder is %dp0% (npm, yarn) or %~dp0 (pnpm).
        let targets = batch_targets(&text);
        // A script, run by the node next to the batch file or else the one on the PATH. (Its batch
        // file names node.exe too, so that is no program to take.)
        if let Some(script) = targets.iter().find(|target| is_script(target)) {
            let beside = win_join(&dir, "node.exe");
            let node = if is_file(&beside) { Some(beside) } else { find_node() };
            if let Some(node) = node {
                return plain(node, vec![win_join(&dir, script)], None, file);
            }
        }
        // A program (npm's batch file for Claude Code's own claude.exe).
        let program = targets.iter().find(|target| {
            let lower = target.to_ascii_lowercase();
            lower.ends_with(".exe") && lower != "node.exe" && !lower.ends_with("\\node.exe")
        });
        if let Some(program) = program {
            return plain(win_join(&dir, program), Vec::new(), None, file);
        }
        return plain("cmd.exe".into(), Vec::new(), Some(file.to_owned()), file);
    }
    plain(file.to_owned(), Vec::new(), None, file)
}

/// The arguments for cmd.exe that run `script` (a batch file) with `args`, as one command line.
/// With /s, cmd takes off the quotes around the whole of it, so a path with spaces stays one path
/// (cmd mangles it otherwise). Arguments are quoted where needed; a quote inside one cannot get
/// through cmd intact and is left out.
pub fn cmd_command_line(script: &str, args: &[String]) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_-.:\\/=@+,".contains(c);
    let quote = |arg: &str| {
        if !arg.is_empty() && arg.chars().all(plain) {
            arg.to_owned()
        } else {
            format!("\"{}\"", arg.replace('"', ""))
        }
    };
    let parts: Vec<String> = std::iter::once(script)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect();
    format!("/d /s /c \"{}\"", parts.join(" "))
}

fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len() && text.as_bytes()[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

/// A JavaScript file: `.js`, `.cjs` or `.mjs`.
fn is_script(path: &str) -> bool {
    [".js", ".cjs", ".mjs"]
        .iter()
        .any(|ext| ends_with_ignore_case(path, ext))
}

/// The paths a batch file names relative to its own folder: every `"%dp0%\..."` or
/// `"%~dp0\..."`, without that prefix.
fn batch_targets(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut targets = Vec::new();
    let mut at = 0;
    while let Some(found) = text[at..].find("\"%") {
        let start = at + found;
        at = start + 1;
        let mut next = start + 2;
        if bytes.get(next) == Some(&b'~') {
            next += 1;
        }
        if !bytes
            .get(next..next + 3)
            .is_some_and(|name| name.eq_ignore_ascii_case(b"dp0"))
        {
            continue;
        }
        next += 3;
        if bytes.get(next) == Some(&b'%') {
            next += 1;
        }
        if bytes.get(next) != Some(&b'\\') {
            continue;
        }
        next += 1;
        let Some(length) = text[next..].find('"') else { continue };
        if length == 0 {
            continue;
        }
        targets.push(text[next..next + length].to_owned());
        at = next + length + 1;
    }
    targets
}

/// The folder of a Windows path.
fn win_dirname(path: &str) -> String {
    match path.rfind(['\\', '/']) {
        Some(index) => path[..index].to_owned(),
        None => ".".to_owned(),
    }
}

/// Joins Windows paths as Node's `path.win32.join` does: backslashes, and `.` and `..` resolved.
fn win_join(dir: &str, rest: &str) -> String {
    let joined = format!("{dir}\\{rest}").replace('/', "\\");
    // What comes before the folders: a drive ("C:"), or the two backslashes of a share.
    let (prefix, tail) = if joined.len() >= 2 && joined.as_bytes()[1] == b':' {
        joined.split_at(2)
    } else if let Some(tail) = joined.strip_prefix("\\\\") {
        ("\\", tail)
    } else {
        ("", joined.as_str())
    };
    let rooted = tail.starts_with('\\') || prefix == "\\";
    let mut parts: Vec<&str> = Vec::new();
    for part in tail.split('\\') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            _ => parts.push(part),
        }
    }
    format!("{prefix}{}{}", if rooted { "\\" } else { "" }, parts.join("\\"))
}

/// Joins POSIX paths as Node's `path.posix.join` does.
fn posix_join(dir: &str, rest: &str) -> String {
    let joined = format!("{dir}/{rest}");
    let rooted = joined.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            _ => parts.push(part),
        }
    }
    format!("{}{}", if rooted { "/" } else { "" }, parts.join("/"))
}
