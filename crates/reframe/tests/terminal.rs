//! A terminal window with claude dropped onto reframe: what the Electron version's
//! `e2e/terminal-window.spec.ts` checked. A fake claude runs under cmd in a real console window
//! (or a Windows Terminal tab, if that is the default terminal). The tests do what the release of
//! a dragged window, or a terminal tab torn out over reframe, does (the test channel), answer the
//! question, and check that the session moved into reframe and that the console, with only cmd
//! left, was closed by typing "exit". One claude runs in WSL (the stand-in), in a shell that the
//! stand-in wsl.exe plays. Needs `node` on the PATH; opens console windows for a moment.

mod common;

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{App, Setup, args, node, wait_for};
use serde_json::{Value, json};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn plain(path: &Path) -> String {
    path.canonicalize()
        .unwrap()
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_owned()
}

fn alive(pid: u64) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
}

fn parent_of(pid: u64) -> u64 {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").ParentProcessId"),
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().parse().unwrap_or(0)
}

/// The pid of the claude registered under this session id in a registry folder.
fn registered(registry: &Path, session_id: &str) -> Option<u64> {
    std::fs::read_dir(registry).ok()?.flatten().find_map(|file| {
        let text = std::fs::read_to_string(file.path()).ok()?;
        let entry: Value = serde_json::from_str(&text).ok()?;
        (entry["sessionId"] == session_id)
            .then(|| entry["pid"].as_u64())
            .flatten()
    })
}

/// A console window with an interactive cmd that runs `command`, like typing it at the prompt.
/// (`start` gives it a console; a batch file would answer Ctrl+C with "Terminate batch job?".)
fn start_terminal(setup: &Setup, cwd: &Path, command: &str, extra_env: &[(&str, &str)]) {
    let line = format!("/c start \"reframe test terminal\" cmd.exe /s /k \"{command}\"");
    let mut start = Command::new("cmd");
    start
        .raw_arg(line)
        .current_dir(cwd)
        .env("REFRAME_CLAUDE_HOME", setup.path("claude-home"))
        .env("REFRAME_FAKE_LOG", setup.path("fake.log"))
        .env("REFRAME_WSL_SHARE", setup.path("wsl"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (name, value) in extra_env {
        start.env(name, value);
    }
    assert!(start.status().unwrap().success());
}

/// Ends the outside terminal with the test, also one that failed.
struct Terminal {
    shell: u64,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.shell != 0 && alive(self.shell) {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.shell.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// Claude (the fake) named "outside" under cmd in its own console; once it is registered: its
/// session id, its pid and the shell's.
fn outside_terminal(setup: &Setup) -> (String, u64, Terminal) {
    let session_id = uuid::Uuid::new_v4().to_string();
    let no_hooks = setup.path("no-hooks.json");
    std::fs::write(&no_hooks, "{}").unwrap();
    let fake = plain(&root().join("test").join("fake-claude").join("fake-claude.mjs"));
    let command = format!(
        "\"{}\" \"{fake}\" --session-id {session_id} --name outside --settings \"{}\"",
        node(),
        no_hooks.display()
    );
    start_terminal(setup, &setup.path("project"), &command, &[]);
    let registry = setup.path("claude-home").join("sessions");
    let pid = wait_for("the outside claude to register", || registered(&registry, &session_id));
    let shell = parent_of(pid);
    assert!(shell > 0);
    (session_id, pid, Terminal { shell })
}

fn starts_of(setup: &Setup, session_id: &str) -> usize {
    setup
        .fake_log()
        .iter()
        .filter(|entry| entry["event"] == "start" && args(entry).contains(&session_id))
        .count()
}

fn exited(setup: &Setup, pid: u64) -> bool {
    setup
        .fake_log()
        .iter()
        .any(|entry| entry["event"] == "exit" && entry["pid"].as_u64() == Some(pid))
}

/// The middle of the right half of the first tile, where a drop goes next to it.
fn right_of_first_tile(app: &mut App) -> (f64, f64) {
    let rect = wait_for("a tile", || {
        app.cmd(json!({ "cmd": "placement" }))["tiles"][0]["rect"]
            .as_array()
            .cloned()
    });
    let number = |at: usize| rect[at].as_f64().unwrap();
    (number(0) + number(2) - 20.0, number(1) + number(3) / 2.0)
}

fn question(app: &mut App) -> Value {
    wait_for("the question", || {
        let question = app.cmd(json!({ "cmd": "window" }))["terminalQuestion"].clone();
        (!question.is_null()).then_some(question)
    })
}

fn record_of(app: &mut App, claude_id: &str) -> Option<String> {
    let state = app.state();
    state["sessions"]
        .as_object()?
        .values()
        .find(|record| record["claudeSessionId"] == claude_id)
        .and_then(|record| record["id"].as_str().map(str::to_owned))
}

#[test]
fn a_terminal_window_dropped_onto_reframe_hands_its_claude_over_and_closes() {
    let setup = Setup::new();
    let (session_id, pid, terminal) = outside_terminal(&setup);
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let alpha = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&alpha, "idle");
    let (x, y) = right_of_first_tile(&mut app);

    let hwnd = app.cmd(json!({ "cmd": "terminal_window_of", "pid": pid }));
    assert!(hwnd.is_i64(), "{hwnd}");
    // Released twice in a row (some terminals report the end of a move twice): one question.
    let drop = json!({ "cmd": "drop_terminal_window", "hwnd": hwnd, "x": x, "y": y });
    app.cmd(drop.clone());
    app.cmd(drop);
    let candidates = question(&mut app);
    assert_eq!(candidates.as_array().unwrap().len(), 1);
    assert_eq!(candidates[0]["name"], "outside");
    assert_eq!(candidates[0]["preselected"], true);
    app.cmd(json!({ "cmd": "terminal_answer", "ids": [session_id] }));

    // claude left the terminal and continues in reframe, next to alpha, under its name.
    wait_for("claude to leave the terminal", || exited(&setup, pid).then_some(()));
    let id = wait_for("it in reframe", || record_of(&mut app, &session_id));
    app.wait_status(&id, "idle");
    let layout = app.state()["windows"][0]["layout"].clone();
    assert_eq!(layout["kind"], "split", "{layout}");
    assert_eq!(layout["children"][1]["sessionId"], id.as_str());
    assert_eq!(app.record(&id)["name"], "outside");
    assert_eq!(starts_of(&setup, &session_id), 2);

    // Only cmd was left in that console: reframe typed "exit" and the console went away.
    wait_for("the console to go", || (!alive(terminal.shell)).then_some(()));
    let log = setup.log();
    assert!(log.contains("typing exit"), "{log}");
    assert!(log.contains("repeated drop of the same terminal window ignored"));
    app.quit();
}

#[test]
fn a_terminal_window_with_a_claude_in_wsl_hands_it_over_and_its_shells_get_exit() {
    let setup = Setup::new();
    let share = setup.path("wsl");
    std::fs::create_dir_all(share.join("Fake").join("home").join("ann").join("proj")).unwrap();
    let session_id = uuid::Uuid::new_v4().to_string();
    let fake_wsl = plain(&root().join("test").join("fake-wsl").join("fake-wsl.mjs"));
    let command = format!(
        "\"{}\" \"{fake_wsl}\" -d Fake --cd /home/ann/proj --exec env PATH=/usr/bin /usr/local/bin/claude --session-id {session_id} --name outside",
        node()
    );
    start_terminal(
        &setup,
        setup.path("").as_path(),
        &command,
        &[("REFRAME_FAKE_WSL_SHELL", "1")],
    );
    let registry = share
        .join("Fake")
        .join("home")
        .join("ann")
        .join(".claude")
        .join("sessions");
    let pid = wait_for("the claude in wsl to register", || registered(&registry, &session_id));
    let wsl_pid = parent_of(pid);
    let terminal = Terminal {
        shell: parent_of(wsl_pid),
    };
    assert!(terminal.shell > 0);

    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let alpha = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&alpha, "idle");
    let (x, y) = right_of_first_tile(&mut app);
    // The tab's console is wsl.exe's (the stand-in's).
    let hwnd = app.cmd(json!({ "cmd": "terminal_window_of", "pid": wsl_pid }));
    assert!(hwnd.is_i64(), "{hwnd}");
    app.cmd(json!({ "cmd": "drop_terminal_window", "hwnd": hwnd, "x": x, "y": y }));
    let candidates = question(&mut app);
    let proj = share.join("Fake").join("home").join("ann").join("proj");
    assert_eq!(candidates[0]["cwd"], json!(proj.to_string_lossy()));
    app.cmd(json!({ "cmd": "terminal_answer", "ids": [session_id] }));

    let id = wait_for("it in reframe", || record_of(&mut app, &session_id));
    app.wait_status(&id, "idle");
    assert_eq!(app.record(&id)["cwd"], json!(proj.to_string_lossy()));
    // The shell in WSL got "exit", then cmd did: the console is gone.
    wait_for("the console to go", || (!alive(terminal.shell)).then_some(()));
    let log = setup.log();
    assert!(log.contains(r#""shell":"wsl""#), "{log}");
    app.quit();
}

#[test]
fn a_terminal_tab_torn_out_over_reframe_is_taken_over_too() {
    let setup = Setup::new();
    let (session_id, pid, terminal) = outside_terminal(&setup);
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let alpha = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&alpha, "idle");
    let (x, y) = right_of_first_tile(&mut app);
    let hwnd = app.cmd(json!({ "cmd": "terminal_window_of", "pid": pid }));
    // The window appears where the tab was let go.
    app.cmd(json!({ "cmd": "terminal_appeared", "hwnd": hwnd, "x": x, "y": y }));
    question(&mut app);
    // Cancelled: nothing happens.
    app.cmd(json!({ "cmd": "terminal_answer", "ids": null }));
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(record_of(&mut app, &session_id).is_none());
    assert!(!exited(&setup, pid));
    // Again, taken over.
    std::thread::sleep(std::time::Duration::from_millis(2000));
    app.cmd(json!({ "cmd": "terminal_appeared", "hwnd": hwnd, "x": x, "y": y }));
    question(&mut app);
    app.cmd(json!({ "cmd": "terminal_answer", "ids": [session_id] }));
    let id = wait_for("it in reframe", || record_of(&mut app, &session_id));
    app.wait_status(&id, "idle");
    wait_for("the console to go", || (!alive(terminal.shell)).then_some(()));
    app.quit();
}
