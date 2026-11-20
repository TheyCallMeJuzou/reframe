//! Sessions in WSL folders, with the stand-ins for wsl.exe and claude: what the Electron version's
//! `e2e/wsl.spec.ts` checked. A session in a WSL folder runs its claude in that distribution
//! through wsl.exe; no hooks reach reframe from there, so its status comes from Claude Code's
//! registry there. Needs `node` on the PATH.

mod common;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use common::{Setup, args, node, wait_for};
use serde_json::{Value, json};

/// The stand-in WSL with two distributions: Fake, and NoClaude without claude. The project folder
/// in Fake.
fn wsl_setup(setup: &Setup) -> PathBuf {
    let share = setup.path("wsl");
    let folder = share.join("Fake").join("home").join("ann").join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(share.join("NoClaude").join("home").join("ann").join("proj")).unwrap();
    std::fs::write(share.join("NoClaude").join("no-claude"), "").unwrap();
    folder
}

fn claude_home(setup: &Setup) -> PathBuf {
    setup.path("wsl").join("Fake").join("home").join("ann").join(".claude")
}

/// What was typed into a session in Fake, as its transcript has it.
fn prompts(setup: &Setup, claude_id: &str) -> Vec<String> {
    let file = claude_home(setup)
        .join("projects")
        .join("fake-project")
        .join(format!("{claude_id}.jsonl"));
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| {
            entry["prompt"]
                .as_str()
                .map(|prompt| prompt.trim().trim_matches('\'').to_owned())
        })
        .collect()
}

#[test]
fn a_session_in_a_wsl_folder_runs_there_with_linux_paths_and_resumes() {
    let setup = Setup::new();
    let folder = wsl_setup(&setup);
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": folder, "name": "linux" }));
    assert_eq!(created["ok"], true, "{created}");
    let id = created["session"].as_str().unwrap().to_owned();
    // The distribution is asked first (a moment), then claude runs there.
    app.wait_status(&id, "idle");
    let sessions = app.cmd(json!({ "cmd": "sessions" }));
    assert_eq!(sessions[&id]["cwdLabel"], "Fake ~/proj");
    let start = setup
        .fake_log()
        .into_iter()
        .find(|entry| entry["event"] == "start")
        .unwrap();
    assert!(!args(&start).contains(&"--settings"), "no hooks from WSL");
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();

    // A file dropped onto the tile is typed in as its Linux path.
    let note = setup.path("note.txt");
    std::fs::write(&note, "hello").unwrap();
    app.cmd(json!({ "cmd": "drop_files", "session": id, "paths": [note] }));
    app.write(&id, "\r");
    let typed = wait_for("the path to be typed", || {
        let prompts = prompts(&setup, &claude_id);
        (prompts.len() == 1).then(|| prompts[0].clone())
    });
    assert!(typed.starts_with("/mnt/") && typed.ends_with("/note.txt"), "{typed}");
    // Busy, then done: told by the registry (the tests turn notifications off; the log says).
    wait_for("done from the registry", || {
        setup.log().contains(r#""kind":"done""#).then_some(())
    });
    // Used, as the registry told (no hook did), and its Linux process is known.
    wait_for("it to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    assert!(app.record(&id)["linuxPid"].is_u64());

    // After a restart it resumes there.
    app.quit();
    let mut app = setup.launch();
    app.wait_status(&id, "idle");
    let starts: Vec<Value> = setup
        .fake_log()
        .into_iter()
        .filter(|entry| entry["event"] == "start")
        .collect();
    assert_eq!(starts.len(), 2);
    assert!(args(&starts[1]).contains(&"--resume"));
    app.write(&id, "after the restart\r");
    wait_for("the prompt", || (prompts(&setup, &claude_id).len() == 2).then_some(()));
    assert_eq!(prompts(&setup, &claude_id)[1], "after the restart");
    app.quit();
}

#[test]
fn a_wsl_folder_whose_distribution_has_no_claude_says_so() {
    let setup = Setup::new();
    wsl_setup(&setup);
    let mut app = setup.launch();
    let folder = setup.path("wsl").join("NoClaude").join("home").join("ann").join("proj");
    let pending = app.cmd(json!({ "cmd": "new", "cwd": folder, "check": true }));
    assert_eq!(pending["pending"], true);
    let answer = wait_for("the answer", || {
        let answer = app.cmd(json!({ "cmd": "new_result" }));
        (!answer.is_null()).then_some(answer)
    });
    assert_eq!(
        answer,
        json!({ "ok": false, "error": "claudeNotFound", "distro": "NoClaude" })
    );
    assert_eq!(app.state()["sessions"], json!({}));
    app.quit();
}

/// A claude in Fake running in a terminal of its own, outside reframe.
struct Outside(Child);

impl Drop for Outside {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

#[test]
fn the_session_list_shows_wsl_sessions_takes_one_over_and_starts_a_distribution() {
    let setup = Setup::new();
    wsl_setup(&setup);
    let share = setup.path("wsl");
    // Known from an earlier run to have Claude Code, but not running now.
    std::fs::create_dir_all(share.join("Stopped").join("home").join("ann")).unwrap();
    std::fs::write(share.join("Stopped").join("stopped"), "").unwrap();
    std::fs::create_dir_all(setup.path("user-data")).unwrap();
    let state = json!({
        "version": 1,
        "sessions": {},
        "windows": [],
        "recentFolders": [],
        "preferences": { "nameFromFolder": true, "language": "en", "fontSize": 13 },
        "workspaces": [],
        "wslDistros": ["Stopped"],
    });
    std::fs::write(setup.path("user-data").join("state.json"), state.to_string()).unwrap();
    // In Fake: a past session, and one running in a terminal of its own.
    let home = claude_home(&setup);
    let past = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(share.join("Fake").join("home").join("ann").join("old")).unwrap();
    let projects = home.join("projects").join("-home-ann-old");
    std::fs::create_dir_all(&projects).unwrap();
    let lines = [
        json!({ "type": "user", "cwd": "/home/ann/old", "message": { "content": "tidy up the logs" } }),
        json!({ "type": "ai-title", "aiTitle": "Log cleanup" }),
    ];
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(projects.join(format!("{past}.jsonl")), text).unwrap();
    let running = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(share.join("Fake").join("home").join("ann").join("other")).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let fake_wsl = root.join("test").join("fake-wsl").join("fake-wsl.mjs");
    let outside = Outside(
        Command::new(node())
            .arg(fake_wsl)
            .args([
                "-d",
                "Fake",
                "--cd",
                "/home/ann/other",
                "--exec",
                "env",
                "PATH=/usr/bin",
                "claude",
            ])
            .args(["--session-id", &running, "--name", "outside"])
            .env("REFRAME_WSL_SHARE", &share)
            .env("REFRAME_FAKE_LOG", setup.path("fake.log"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let registry = home.join("sessions");
    wait_for("the outside claude's entry", || {
        (std::fs::read_dir(&registry).map(|files| files.count()).unwrap_or(0) == 1).then_some(())
    });

    let mut app = setup.launch();
    app.cmd(json!({ "cmd": "sidebar" }));
    let list = wait_for("the list with WSL's sessions", || {
        let list = app.cmd(json!({ "cmd": "list" }));
        (list["running"].as_array().is_some_and(|running| running.len() == 1)
            && list["found"].as_array().is_some_and(|found| found.len() == 1))
        .then_some(list)
    });
    assert_eq!(list["running"][0]["name"], "outside");
    let other = share.join("Fake").join("home").join("ann").join("other");
    assert_eq!(list["running"][0]["cwd"], json!(other.to_string_lossy()));
    assert_eq!(list["found"][0]["title"], "Log cleanup");
    assert_eq!(list["wslStopped"], json!(["Stopped"]));

    // Started from the list, it runs (kept running by reframe) and its note goes.
    app.cmd(json!({ "cmd": "wsl_start", "distro": "Stopped" }));
    wait_for("the note to go", || {
        (app.cmd(json!({ "cmd": "list" }))["wslStopped"] == json!([])).then_some(())
    });

    // The past one resumes in its WSL folder.
    app.cmd(json!({ "cmd": "row_click", "row": format!("claude:{past}") }));
    wait_for("it to resume", || {
        setup
            .fake_log()
            .iter()
            .any(|entry| {
                entry["event"] == "start" && args(entry).windows(2).any(|pair| pair == ["--resume", past.as_str()])
            })
            .then_some(())
    });

    // The running one is taken over after asking: ended there, resumed here.
    app.cmd(json!({ "cmd": "row_click", "row": format!("claude:{running}") }));
    let question = wait_for("the question", || {
        let question = app.cmd(json!({ "cmd": "window" }))["question"].clone();
        (!question.is_null()).then_some(question)
    });
    assert_eq!(question["message"], "outside is running in another terminal.");
    app.cmd(json!({ "cmd": "answer", "yes": true }));
    let id = wait_for("it to be reframe's", || {
        let state = app.state();
        state["sessions"]
            .as_object()
            .unwrap()
            .values()
            .find(|record| record["claudeSessionId"] == running.as_str())
            .map(|record| record["id"].as_str().unwrap().to_owned())
    });
    app.wait_status(&id, "idle");
    let record = app.record(&id);
    assert_eq!(record["cwd"], json!(other.to_string_lossy()));
    assert_eq!(record["name"], "outside", "a name the user gave survives");
    drop(outside);
    app.quit();
}
