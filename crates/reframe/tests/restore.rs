//! Starting again: what the Electron version's `e2e/restore.spec.ts`, `e2e/launcher.spec.ts`,
//! `e2e/shortcuts.spec.ts` (F11), `e2e/stale-registry.spec.ts`, `e2e/start-failure.spec.ts` and
//! `e2e/state-file.spec.ts` checked, where the native tests had nothing yet.

mod common;

use std::process::{Child, Command, Stdio};

use common::{Setup, args, node, wait_for};
use serde_json::{Value, json};

fn starts(setup: &Setup) -> Vec<Value> {
    setup
        .fake_log()
        .into_iter()
        .filter(|entry| entry["event"] == "start")
        .collect()
}

#[test]
fn claude_behind_an_unknown_launcher_in_a_folder_with_a_space_runs_with_its_status() {
    let setup = Setup::new();
    let tools = setup.path("My Tools");
    std::fs::create_dir(&tools).unwrap();
    let launcher = tools.join("claude.cmd");
    let fake = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("fake-claude")
        .join("fake-claude.mjs")
        .canonicalize()
        .unwrap();
    let fake = fake.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    std::fs::write(&launcher, format!("@\"{}\" \"{fake}\" %*\r\n", node())).unwrap();
    let mut command = setup.command();
    command.env("REFRAME_CLAUDE_BIN", &launcher);
    let mut app = setup.launch_command(command, &[]);
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "my task" }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    let start = wait_for("the start", || starts(&setup).into_iter().next());
    // The session's process is claude's own (for cleaning up after a crash), not cmd's, once its
    // registry entry was seen.
    wait_for("claude's own process", || {
        (app.record(&id)["pid"] == start["pid"]).then_some(())
    });
    let record = app.record(&id);
    let started = args(&start);
    let claude_id = record["claudeSessionId"].as_str().unwrap();
    assert!(
        started.windows(2).any(|pair| pair == ["--session-id", claude_id]),
        "{started:?}"
    );
    assert!(
        started.windows(2).any(|pair| pair == ["--name", "my task"]),
        "{started:?}"
    );

    app.write(&id, "hello\r");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    app.quit();
    let exits = setup.fake_log().iter().filter(|entry| entry["event"] == "exit").count();
    assert_eq!(exits, 1);
}

#[test]
fn a_worktree_session_started_again_without_a_transcript_runs_in_its_worktree() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({
        "cmd": "new", "cwd": setup.project(), "name": "wt", "worktree": true, "worktreeName": "feat"
    }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    let worktree = setup.path("project").join(".claude").join("worktrees").join("feat");
    wait_for("the worktree", || {
        (app.record(&id)["launch"]["worktreeCreated"] == true).then_some(())
    });
    assert_eq!(
        app.record(&id)["cwd"].as_str().unwrap().to_lowercase(),
        worktree.to_string_lossy().to_lowercase()
    );
    app.quit();

    // Never prompted, so it starts fresh under its id: in the worktree, and without making
    // another one in there.
    let mut app = setup.launch();
    app.wait_status(&id, "idle");
    let all = starts(&setup);
    assert_eq!(all.len(), 2);
    assert!(args(&all[0]).contains(&"--worktree"));
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    assert_eq!(args(&all[1])[..2], ["--session-id", claude_id.as_str()]);
    assert!(!args(&all[1]).contains(&"--worktree"));
    assert!(!worktree.join(".claude").join("worktrees").exists());
    app.quit();
}

#[test]
fn a_session_started_anew_after_its_transcript_was_lost_starts_without_one_after_a_restart_too() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    app.write(&id, "hello\r");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    let lost = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    app.quit();
    std::fs::remove_file(
        setup
            .path("claude-home")
            .join("projects")
            .join("fake-project")
            .join(format!("{lost}.jsonl")),
    )
    .unwrap();

    let mut app = setup.launch();
    let ui = app.ui();
    ui.wait("text", "Claude Code no longer has this session's transcript.");
    ui.click("button", "Start a new session here");
    app.wait_status(&id, "idle");
    assert_ne!(app.record(&id)["claudeSessionId"], json!(lost));
    assert_eq!(app.record(&id)["used"], false);
    app.quit();

    // No prompt in the new one yet: it starts fresh under its id again, instead of "transcript
    // missing".
    let mut app = setup.launch();
    app.wait_status(&id, "idle");
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    let last = starts(&setup).pop().unwrap();
    assert_eq!(args(&last)[..2], ["--session-id", claude_id.as_str()]);
    app.quit();
}

#[test]
fn f11_full_screen_is_kept_after_a_restart() {
    let setup = Setup::new();
    let mut app = setup.launch();
    app.cmd(json!({ "cmd": "key", "keys": "f11" }));
    wait_for("full screen", || {
        (app.cmd(json!({ "cmd": "window" }))["fullscreen"] == true).then_some(())
    });
    // A hint says for a moment how to leave it; a button stays.
    let ui = app.ui();
    ui.wait("text", "Press F11 to exit full screen");
    ui.wait("button", "Exit full screen (F11)");
    ui.wait_gone("text", "Press F11 to exit full screen");
    wait_for("the window to be saved full screen", || {
        (app.state()["windows"][0]["fullscreen"] == true).then_some(())
    });
    app.quit();

    let mut app = setup.launch();
    wait_for("full screen again", || {
        (app.cmd(json!({ "cmd": "window" }))["fullscreen"] == true).then_some(())
    });
    app.cmd(json!({ "cmd": "key", "keys": "f11" }));
    wait_for("not full screen", || {
        (app.cmd(json!({ "cmd": "window" }))["fullscreen"] == false).then_some(())
    });
    app.quit();
}

/// A process that only waits, ended with the test.
struct Waiting(Child);

impl Drop for Waiting {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn waiting() -> Waiting {
    Waiting(
        Command::new(node())
            .args(["-e", "setInterval(() => {}, 1000)"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn alive(pid: u32) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
}

/// When Windows says the process started, as Claude Code's registry has it.
fn process_start(pid: u32) -> String {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {pid}).StartTime.ToUniversalTime().ToFileTimeUtc()"),
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn registry_entries_left_behind_under_another_programs_process_id_are_left_alone() {
    let setup = Setup::new();
    let (orphan, foreign, real) = (waiting(), waiting(), waiting());
    let sessions = setup.path("claude-home").join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let entry = |pid: u32, session_id: &str, proc_start: &str| {
        let text = json!({
            "pid": pid, "sessionId": session_id, "cwd": setup.project(), "procStart": proc_start,
            "startedAt": 1, "status": "idle"
        });
        std::fs::write(sessions.join(format!("{pid}.json")), text.to_string()).unwrap();
    };
    entry(orphan.0.id(), "own-session", "1");
    entry(foreign.0.id(), "foreign-session", "1");
    entry(real.0.id(), "real-session", &process_start(real.0.id()));
    // A reframe session whose last process id is now another program's.
    let user_data = setup.path("user-data");
    std::fs::create_dir_all(&user_data).unwrap();
    let state = json!({
        "version": 1,
        "sessions": { "a": {
            "id": "a", "claudeSessionId": "own-session", "cwd": setup.project(), "launch": {},
            "lifecycle": "ended", "createdAt": 1, "lastActiveAt": 1, "pid": orphan.0.id()
        } },
        "windows": [{ "id": "w", "bounds": null, "maximized": false, "hidden": false, "layout": null, "dock": [] }],
        "recentFolders": []
    });
    std::fs::write(user_data.join("state.json"), state.to_string()).unwrap();

    let mut app = setup.launch();
    let running = wait_for("the list", || {
        let list = app.cmd(json!({ "cmd": "list" }));
        list["running"].as_array().cloned()
    });
    let ids: Vec<&str> = running
        .iter()
        .filter_map(|item| item["claudeSessionId"].as_str())
        .collect();
    assert_eq!(ids, ["real-session"], "{running:?}");
    assert!([orphan.0.id(), foreign.0.id(), real.0.id()].into_iter().all(alive));
    app.quit();
}

/// Starts the app and waits for it to end on its own (it is ended with the test if it does not).
fn run_to_exit(mut command: Command) -> i32 {
    let mut child = Waiting(command.spawn().unwrap());
    let status = wait_for("the app to give up", || child.0.try_wait().unwrap());
    status.code().unwrap_or(-1)
}

#[test]
fn a_start_that_fails_ends_the_process_and_says_why_in_the_log() {
    let setup = Setup::new();
    // The state cannot be read: state.json is a folder.
    std::fs::create_dir_all(setup.path("user-data").join("state.json")).unwrap();
    assert_eq!(run_to_exit(setup.command()), 1);
    let log = setup.log();
    assert!(log.contains("reframe could not start"), "{log}");
    assert!(log.contains("state.json could not be read"), "{log}");
}

#[test]
fn an_unusable_state_file_is_set_aside_and_the_backup_takes_over() {
    let setup = Setup::new();
    let user_data = setup.path("user-data");
    std::fs::create_dir_all(&user_data).unwrap();
    std::fs::write(user_data.join("state.json"), r#"{ "version": 1, "sessions": "#).unwrap();
    let backup = json!({ "version": 1, "sessions": {}, "windows": [], "recentFolders": [setup.project()] });
    std::fs::write(user_data.join("state.json.bak"), backup.to_string()).unwrap();

    let mut app = setup.launch();
    assert_eq!(app.state()["recentFolders"], json!([setup.project()]));
    let aside: Vec<_> = std::fs::read_dir(&user_data)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("state.json.rejected-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(
        std::fs::read_to_string(aside[0].path()).unwrap(),
        r#"{ "version": 1, "sessions": "#
    );
    assert!(setup.log().contains("could not be used, set aside"));
    app.quit();
}
