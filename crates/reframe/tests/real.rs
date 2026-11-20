//! Smoke tests against the real claude CLI (and Claude Code in a real WSL distribution), as the
//! Electron version's `e2e/real-*.spec.ts`. Opt-in: they need Claude Code installed and logged
//! in, and a folder it trusts; each is ignored unless asked for:
//!
//! ```text
//! set REFRAME_E2E_FOLDER=<a folder the real claude trusts>
//! set REFRAME_E2E_WSL=Ubuntu
//! set REFRAME_E2E_WSL_FOLDER=/home/<user>/<a folder Claude Code trusts there>
//! cargo test -p reframe --test real -- --ignored --test-threads=1
//! ```
//!
//! What they send to Claude: the hooks test two tiny prompts ("Reply with exactly: OK", with
//! haiku), the WSL test one ("Reply with only the word OK"); the others none. The fullscreen test
//! (with `"tui": "fullscreen"` in ~/.claude/settings.json) borrows the system clipboard and puts
//! its text back; it is skipped when the clipboard holds anything else. The terminal tests open a
//! console window, or a Windows Terminal window, of their own. Pictures of the windows land in
//! `target/real-shots`.

mod common;

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{App, Setup};
use serde_json::{Value, json};

const STARTING: Duration = Duration::from_secs(60);
const ANSWERING: Duration = Duration::from_secs(120);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} (see the top of tests/real.rs)"))
}

fn wait<T>(what: &str, within: Duration, mut look: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + within;
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The variables that tell a program it runs inside Claude Code: none of them for what these
/// tests start.
fn claude_variables() -> Vec<String> {
    std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| key.to_uppercase().starts_with("CLAUDE"))
        .collect()
}

/// The app as a user has it: the real claude, the real WSL, Claude Code's own folder.
fn real(setup: &Setup, locale: &str, notifications: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reframe-native"));
    for key in claude_variables() {
        command.env_remove(key);
    }
    command
        .env_remove("NO_COLOR")
        .env("REFRAME_E2E", "1")
        .env("REFRAME_USER_DATA", setup.path("user-data"))
        .env("REFRAME_LOCALE", locale)
        .stdin(Stdio::null());
    if !notifications {
        command.env("REFRAME_DISABLE_NOTIFICATIONS", "1");
    }
    command
}

fn shot(name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let folder = root.join("target").join("real-shots").join(name);
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-File"])
        .arg(root.join("scripts").join("capture.ps1"))
        .arg(folder)
        .status();
}

fn wait_status(app: &mut App, session: &str, status: &str, within: Duration) {
    wait(&format!("status {status}"), within, || {
        (app.status(session) == status).then_some(())
    });
}

fn new_session(app: &mut App, folder: &str, name: &str, model: Option<&str>) -> String {
    let mut command = json!({ "cmd": "new", "cwd": folder, "name": name });
    if let Some(model) = model {
        command["model"] = json!(model);
    }
    let created = app.cmd(command);
    let id = created["session"]
        .as_str()
        .unwrap_or_else(|| panic!("{created}"))
        .to_owned();
    wait_status(app, &id, "idle", STARTING);
    id
}

fn claude_home() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap()).join(".claude")
}

/// Claude Code's registry entry with this name, once it is there.
fn registered(name: &str) -> Option<Value> {
    std::fs::read_dir(claude_home().join("sessions"))
        .ok()?
        .flatten()
        .filter_map(|file| std::fs::read_to_string(file.path()).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .find(|entry| entry["name"] == name)
}

fn alive(pid: u64) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
}

fn kill_tree(pid: u64) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[test]
#[ignore = "the real claude: see the top of tests/real.rs"]
fn real_claude_two_sessions_restart_and_both_resume() {
    let folder = env("REFRAME_E2E_FOLDER");
    let setup = Setup::new();
    let mut app = setup.launch_command(real(&setup, "de", false), &[]);
    let links = new_session(&mut app, &folder, "links", None);
    let rechts = new_session(&mut app, &folder, "rechts", None);
    std::thread::sleep(Duration::from_millis(2500));
    shot("real-1-two-sessions");
    app.cmd(json!({ "cmd": "layout", "action": "collapse", "session": rechts }));
    std::thread::sleep(Duration::from_millis(800));
    shot("real-2-collapsed");
    app.quit();

    let mut app = setup.launch_command(real(&setup, "de", false), &[]);
    wait_status(&mut app, &links, "idle", STARTING);
    app.cmd(json!({ "cmd": "layout", "action": "expand", "session": rechts }));
    wait_status(&mut app, &rechts, "idle", STARTING);
    std::thread::sleep(Duration::from_millis(2500));
    shot("real-3-after-restart");
    app.quit();
}

#[test]
#[ignore = "the real claude: see the top of tests/real.rs"]
fn real_claude_a_session_running_in_another_terminal_is_taken_over() {
    let folder = env("REFRAME_E2E_FOLDER");
    let setup = Setup::new();
    let name = format!(
        "takeover-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    // claude in a console window of its own, as in another terminal.
    let mut outside = Command::new("cmd");
    outside
        .raw_arg(format!(
            "/c start \"reframe real takeover\" /min cmd /c claude --name {name} --model haiku"
        ))
        .current_dir(&folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for key in claude_variables() {
        outside.env_remove(key);
    }
    assert!(outside.status().unwrap().success());
    let entry = wait("claude in the other terminal", STARTING, || registered(&name));
    let pid = entry["pid"].as_u64().unwrap();
    let session_id = entry["sessionId"].as_str().unwrap().to_owned();

    let result = std::panic::catch_unwind(|| {
        let mut app = setup.launch_command(real(&setup, "en", false), &[]);
        app.cmd(json!({ "cmd": "sidebar" }));
        let row = format!("claude:{session_id}");
        wait("its row in the list", STARTING, || {
            (app.cmd(json!({ "cmd": "row_click", "row": row }))["ok"] == true).then_some(())
        });
        // reframe asks before it ends claude over there.
        wait("the question", STARTING, || {
            let question = app.cmd(json!({ "cmd": "window" }))["question"].clone();
            (!question.is_null()).then_some(())
        });
        app.cmd(json!({ "cmd": "answer", "yes": true }));
        wait("claude to leave the other terminal", STARTING, || {
            (!alive(pid)).then_some(())
        });
        let id = wait("it in reframe", STARTING, || {
            app.state()["sessions"].as_object().and_then(|sessions| {
                sessions
                    .values()
                    .find(|record| record["claudeSessionId"] == session_id.as_str())
                    .and_then(|record| record["id"].as_str().map(str::to_owned))
            })
        });
        wait_status(&mut app, &id, "idle", STARTING);
        std::thread::sleep(Duration::from_millis(2500));
        shot("real-4-takeover");
        app.quit();
    });
    if alive(pid) {
        kill_tree(pid);
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
#[ignore = "the real claude, two tiny prompts: see the top of tests/real.rs"]
fn real_claude_hooks_report_a_prompt_and_its_end_and_trigger_notifications() {
    let folder = env("REFRAME_E2E_FOLDER");
    let setup = Setup::new();
    let mut app = setup.launch_command(real(&setup, "en", true), &[]);
    let id = new_session(&mut app, &folder, "hooks", Some("haiku"));
    // One tiny prompt: UserPromptSubmit and Stop reach reframe.
    app.write(&id, "Reply with exactly: OK");
    std::thread::sleep(Duration::from_millis(300));
    app.write(&id, "\r");
    wait("the end of the answer", ANSWERING, || {
        setup.log().contains(r#""event":"Stop""#).then_some(())
    });
    let log = setup.log();
    assert!(log.contains(r#""event":"UserPromptSubmit""#), "{log}");
    assert!(
        log.contains("notification shown") || log.contains("notification skipped"),
        "{log}"
    );
    // With the window away, the next answer is announced.
    let window = app.state()["windows"][0]["id"].as_str().unwrap().to_owned();
    app.cmd(json!({ "cmd": "close", "window": window }));
    app.write(&id, "Reply with exactly: OK again");
    std::thread::sleep(Duration::from_millis(300));
    app.write(&id, "\r");
    wait("the notification", ANSWERING, || {
        setup.log().contains("notification shown").then_some(())
    });
    app.quit();
}

fn powershell(command: &str) -> String {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", command])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}

#[test]
#[ignore = "the real claude in its fullscreen mode: see the top of tests/real.rs"]
fn real_claude_fullscreen_a_right_click_pastes_once_and_shift_right_click_is_reframes() {
    let folder = env("REFRAME_E2E_FOLDER");
    let settings = std::fs::read_to_string(claude_home().join("settings.json")).unwrap_or_default();
    if !settings.contains(r#""tui": "fullscreen""#) && !settings.contains(r#""tui":"fullscreen""#) {
        eprintln!("skipped: needs \"tui\": \"fullscreen\" in ~/.claude/settings.json");
        return;
    }
    // Only text on the clipboard is put back: with anything else there, the test leaves it alone.
    let only_text = powershell(
        "Add-Type -AssemblyName System.Windows.Forms; $c = [Windows.Forms.Clipboard]; \
         if ($c::ContainsImage() -or $c::ContainsFileDropList() -or $c::ContainsAudio()) { 'no' } else { 'yes' }",
    );
    if only_text != "yes" {
        eprintln!("skipped: the clipboard holds more than text");
        return;
    }
    let before = powershell("[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-Clipboard -Raw");
    let marker = format!("CLAUDE-PASTE-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let set_clipboard = |text: &str| {
        let mut child = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::InputEncoding = [Text.Encoding]::UTF8; Set-Clipboard -Value ([Console]::In.ReadToEnd())",
            ])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        child.wait().unwrap();
    };
    set_clipboard(&marker);
    let result = std::panic::catch_unwind(|| {
        let setup = Setup::new();
        let mut app = setup.launch_command(real(&setup, "en", false), &[]);
        let id = new_session(&mut app, &folder, "fullscreen", None);
        // The fullscreen program draws itself and turns the mouse on.
        std::thread::sleep(Duration::from_secs(4));
        shot("real-fullscreen-0-started");
        let (cols, rows) = (40, 25);
        let at = wait("the cell", STARTING, || {
            let at = app.cmd(json!({ "cmd": "cell", "session": id, "col": cols, "row": rows }));
            Some((at["x"].as_f64()?, at["y"].as_f64()?))
        });
        let click = |app: &mut App, shift: bool| {
            for kind in ["down", "up"] {
                app.cmd(json!({
                    "cmd": "mouse", "type": kind, "x": at.0, "y": at.1, "button": "right", "shift": shift
                }));
            }
        };
        // reframe's view of the clipboard is the stand-in; Claude Code reads the real one.
        let reframes = "REFRAME-PASTE";
        app.cmd(json!({ "cmd": "clipboard", "text": reframes }));
        click(&mut app, false);
        wait("Claude Code's paste", STARTING, || {
            app.screen(&id).contains(&marker).then_some(())
        });
        std::thread::sleep(Duration::from_millis(1500));
        let screen = app.screen(&id);
        assert_eq!(screen.matches(&marker).count(), 1, "{screen}");
        assert!(!screen.contains(reframes), "{screen}");
        shot("real-fullscreen-1-right-click");
        click(&mut app, true);
        wait("reframe's paste", STARTING, || {
            app.screen(&id).contains(reframes).then_some(())
        });
        shot("real-fullscreen-2-shift-right-click");
        app.quit();
    });
    set_clipboard(&before);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn wsl_folder() -> (String, String, String) {
    let distro = env("REFRAME_E2E_WSL");
    let linux = env("REFRAME_E2E_WSL_FOLDER");
    let windows = format!(r"\\wsl.localhost\{distro}{}", linux.replace('/', r"\"));
    (distro, linux, windows)
}

#[test]
#[ignore = "Claude Code in a real WSL distribution, one short prompt: see the top of tests/real.rs"]
fn real_claude_in_wsl_start_a_prompt_restart_resume() {
    let (distro, _, folder) = wsl_folder();
    let setup = Setup::new();
    let mut app = setup.launch_command(real(&setup, "en", false), &[]);
    let id = new_session(&mut app, &folder, "wsl", None);
    let label = app.cmd(json!({ "cmd": "sessions" }))[&id]["cwdLabel"].clone();
    assert!(label.as_str().is_some_and(|label| label.contains(&distro)), "{label}");
    std::thread::sleep(Duration::from_secs(2));
    shot("real-wsl-1-started");
    // One short prompt: busy, then idle again, from Claude Code's registry alone.
    app.write(&id, "Reply with only the word OK");
    std::thread::sleep(Duration::from_millis(300));
    app.write(&id, "\r");
    wait_status(&mut app, &id, "busy", Duration::from_secs(15));
    wait_status(&mut app, &id, "idle", ANSWERING);
    shot("real-wsl-2-answered");
    app.quit();

    let mut app = setup.launch_command(real(&setup, "en", false), &[]);
    wait_status(&mut app, &id, "idle", STARTING);
    std::thread::sleep(Duration::from_millis(2500));
    shot("real-wsl-3-resumed");
    app.quit();
}

/// The wsl.exe processes whose command line holds `needle`.
fn wsl_pids(needle: &str) -> Vec<u64> {
    powershell(&format!(
        "(Get-CimInstance Win32_Process -Filter \"Name='wsl.exe'\" | Where-Object {{ $_.CommandLine -like '*{needle}*' }}).ProcessId"
    ))
    .split_whitespace()
    .filter_map(|pid| pid.parse().ok())
    .collect()
}

#[test]
#[ignore = "Claude Code in a real WSL distribution, typed into a Windows Terminal window: see the top of tests/real.rs"]
fn real_claude_typed_into_a_wsl_terminal_tab_is_dropped_onto_reframe_and_taken_over() {
    let (distro, linux, _) = wsl_folder();
    let session_id = uuid::Uuid::new_v4().to_string();
    // A Windows Terminal window of its own (never a tab in one of the user's): bash in the
    // distribution, claude typed into it, the shell left at its prompt once claude ends. (No ";":
    // Windows Terminal splits its command line there.)
    Command::new("wt.exe")
        .args([
            "-w", "new", "wsl.exe", "-d", &distro, "--cd", &linux, "--", "bash", "-lic",
        ])
        .arg(format!(
            "claude --session-id {session_id} --name dropped || true && exec bash -l"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    let terminal = wait("the terminal", Duration::from_secs(20), || {
        let pids = wsl_pids(&session_id);
        (!pids.is_empty()).then_some(pids)
    });
    let result = std::panic::catch_unwind(|| {
        let setup = Setup::new();
        let mut app = setup.launch_command(real(&setup, "en", false), &[]);
        let hwnd = app.cmd(json!({ "cmd": "terminal_window_of", "pid": terminal[0] }));
        assert!(hwnd.is_i64(), "{hwnd}");
        let size = app.cmd(json!({ "cmd": "window" }))["size"].clone();
        let (x, y) = (size[0].as_f64().unwrap() / 2.0, size[1].as_f64().unwrap() / 2.0);
        // Dropped once claude is up over there: reframe finds it in that window, by the wsl.exe
        // of its tab.
        let candidates = wait("the question", STARTING, || {
            app.cmd(json!({ "cmd": "drop_terminal_window", "hwnd": hwnd, "x": x, "y": y }));
            std::thread::sleep(Duration::from_secs(2));
            let question = app.cmd(json!({ "cmd": "window" }))["terminalQuestion"].clone();
            (!question.is_null()).then_some(question)
        });
        assert_eq!(candidates.as_array().unwrap().len(), 1, "{candidates}");
        assert_eq!(candidates[0]["name"], "dropped");
        shot("real-wsl-4-dropped");
        app.cmd(json!({ "cmd": "terminal_answer", "ids": [session_id] }));
        let id = wait("it in reframe", STARTING, || {
            app.state()["sessions"].as_object().and_then(|sessions| {
                sessions
                    .values()
                    .find(|record| record["claudeSessionId"] == session_id.as_str())
                    .and_then(|record| record["id"].as_str().map(str::to_owned))
            })
        });
        wait_status(&mut app, &id, "idle", STARTING);
        let label = app.cmd(json!({ "cmd": "sessions" }))[&id]["cwdLabel"].clone();
        assert!(label.as_str().is_some_and(|label| label.contains(&distro)), "{label}");
        // claude ended there, its shell got "exit", and the tab (with its window) went.
        wait("the terminal to close", Duration::from_secs(20), || {
            terminal.iter().all(|pid| !alive(*pid)).then_some(())
        });
        assert!(setup.log().contains("terminal of a claude in wsl found"));
        shot("real-wsl-5-taken-over");
        app.quit();
    });
    for pid in &terminal {
        if alive(*pid) {
            kill_tree(*pid);
        }
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
