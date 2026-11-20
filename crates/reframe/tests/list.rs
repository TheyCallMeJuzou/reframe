//! The session list, sessions running elsewhere, and workspaces, with the fake claude: what the
//! Electron version's `e2e/list.spec.ts`, `elsewhere.spec.ts`, `workspaces.spec.ts` and
//! `last-used.spec.ts` checked. Needs `node` on the PATH.

mod common;

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use common::{App, Setup, args, node, wait_for};
use serde_json::{Value, json};

/// A past session as Claude Code leaves it: a transcript in the fake's project folder (so it
/// resumes for real), last written some minutes ago.
fn past(setup: &Setup, title: &str, minutes_ago: u64) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let folder = setup.path("claude-home").join("projects").join("fake-project");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(format!("{id}.jsonl"));
    let lines = [
        json!({ "type": "user", "cwd": setup.project(), "message": { "content": "some work" } }),
        json!({ "type": "ai-title", "aiTitle": title }),
    ];
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(&file, text).unwrap();
    let when = SystemTime::now() - Duration::from_secs(minutes_ago * 60);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(when)
        .unwrap();
    id
}

/// The list as the first window shows it, once it holds what `ready` looks for.
fn list_with(app: &mut App, what: &str, ready: impl Fn(&Value) -> bool) -> Value {
    wait_for(what, || {
        let list = app.cmd(json!({ "cmd": "list" }));
        ready(&list).then_some(list)
    })
}

fn titles(list: &Value, part: &str) -> Vec<String> {
    list[part]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            item["title"]
                .as_str()
                .or(item["label"].as_str())
                .or(item["name"].as_str())
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

fn starts_of(setup: &Setup, claude_id: &str) -> usize {
    setup
        .fake_log()
        .iter()
        .filter(|entry| entry["event"] == "start" && args(entry).contains(&claude_id))
        .count()
}

fn record_of(app: &mut App, claude_id: &str) -> String {
    let state = app.state();
    state["sessions"]
        .as_object()
        .unwrap()
        .values()
        .find(|record| record["claudeSessionId"] == claude_id)
        .map(|record| record["id"].as_str().unwrap().to_owned())
        .unwrap_or_default()
}

#[test]
fn select_several_open_them_together_and_see_new_ones_pop_up() {
    let setup = Setup::new();
    let alpha = past(&setup, "Alpha task", 10);
    let beta = past(&setup, "Beta task", 20);
    let gamma = past(&setup, "Gamma task", 30);
    let mut app = setup.launch();
    app.cmd(json!({ "cmd": "sidebar" }));
    let list = list_with(&mut app, "the past sessions", |list| {
        list["found"].as_array().is_some_and(|f| f.len() == 3)
    });
    // Newest first.
    assert_eq!(titles(&list, "found"), ["Alpha task", "Beta task", "Gamma task"]);
    assert_eq!(list["app"], json!([]));

    // A search finds what holds it; cleared, all are back.
    app.cmd(json!({ "cmd": "search", "text": "GAMMA" }));
    let list = list_with(&mut app, "the search", |list| {
        list["found"].as_array().is_some_and(|f| f.len() == 1)
    });
    assert_eq!(titles(&list, "found"), ["Gamma task"]);
    app.cmd(json!({ "cmd": "search", "text": "" }));
    list_with(&mut app, "all again", |list| {
        list["found"].as_array().is_some_and(|f| f.len() == 3)
    });

    // Ctrl+click and Shift+click select without opening anything.
    let row = |id: &str| format!("claude:{id}");
    app.cmd(json!({ "cmd": "row_click", "row": row(&alpha), "ctrl": true }));
    app.cmd(json!({ "cmd": "row_click", "row": row(&gamma), "shift": true }));
    assert_eq!(
        app.cmd(json!({ "cmd": "window" }))["selected"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    app.cmd(json!({ "cmd": "row_click", "row": row(&beta), "ctrl": true }));
    assert_eq!(
        app.cmd(json!({ "cmd": "window" }))["selected"],
        json!([row(&alpha), row(&gamma)])
    );
    assert_eq!(app.state()["windows"][0]["layout"], Value::Null);

    // Right-click → "Open in a new group": both resume, framed together.
    let menu = app.cmd(json!({ "cmd": "row_menu", "row": row(&alpha) }));
    assert_eq!(menu["menu"][0]["label"], "Open 2 sessions", "{menu}");
    assert_eq!(
        app.cmd(json!({ "cmd": "menu_click", "label": "Open in a new group" }))["ok"],
        true
    );
    wait_for("both to start", || {
        (starts_of(&setup, &alpha) == 1 && starts_of(&setup, &gamma) == 1).then_some(())
    });
    let layout = app.state()["windows"][0]["layout"].clone();
    assert_eq!(layout["kind"], "group", "{layout}");
    let (alpha_id, gamma_id) = (record_of(&mut app, &alpha), record_of(&mut app, &gamma));
    app.wait_status(&alpha_id, "idle");
    app.wait_status(&gamma_id, "idle");

    // They are reframe's now: listed as such, no longer among the found ones.
    let list = list_with(&mut app, "the opened ones in reframe's part", |list| {
        list["app"].as_array().is_some_and(|app| app.len() == 2)
    });
    assert_eq!(titles(&list, "found"), ["Beta task"]);

    // Beta, and Alpha that is open already, "in a new group in a new window": Alpha moves along
    // without a restart.
    app.cmd(json!({ "cmd": "row_click", "row": row(&beta), "ctrl": true }));
    app.cmd(json!({ "cmd": "row_click", "row": format!("app:{alpha_id}"), "ctrl": true }));
    app.cmd(json!({ "cmd": "row_menu", "row": row(&beta) }));
    assert_eq!(
        app.cmd(json!({ "cmd": "menu_click", "label": "Open in a new group in a new window" }))["ok"],
        true
    );
    let state = wait_for("the new window", || {
        let state = app.state();
        (state["windows"].as_array().unwrap().len() == 2 && starts_of(&setup, &beta) == 1).then_some(state)
    });
    let second = state["windows"][1]["layout"].to_string();
    assert!(second.contains(&alpha_id) && second.contains("group"), "{second}");
    assert!(!state["windows"][0]["layout"].to_string().contains(&alpha_id));
    assert_eq!(starts_of(&setup, &alpha), 1, "moved without a restart");

    // A session started in another terminal while the list is hidden: a dot on the Sessions
    // button; shown, it is highlighted in the list.
    app.cmd(json!({ "cmd": "sidebar" }));
    let outside = run_outside(&setup, None);
    wait_for("the dot on the Sessions button", || {
        (app.cmd(json!({ "cmd": "window" }))["newFound"] == true).then_some(())
    });
    app.cmd(json!({ "cmd": "sidebar" }));
    let list = list_with(&mut app, "the running one", |list| {
        list["running"].as_array().is_some_and(|r| r.len() == 1)
    });
    assert_eq!(list["running"][0]["pid"], outside.pid);
    assert_eq!(app.cmd(json!({ "cmd": "window" }))["newFound"], false);

    drop(outside);
    app.quit();
}

/// The fake claude in a console of its own, outside reframe (as in another terminal): a new
/// session, or `resume` one. `start` gives it a console whose input Ctrl+C can be typed into.
struct Outside {
    pid: u32,
}

fn run_outside(setup: &Setup, resume: Option<&str>) -> Outside {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let fake = root
        .join("test")
        .join("fake-claude")
        .join("fake-claude.mjs")
        .canonicalize()
        .unwrap();
    let fake = fake.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    let no_hooks = setup.path("no-hooks.json");
    std::fs::write(&no_hooks, "{}").unwrap();
    let id = resume.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
    let flag = if resume.is_some() { "--resume" } else { "--session-id" };
    let line = format!(
        "/c start \"\" /min \"{}\" \"{fake}\" {flag} {id} --settings \"{}\"",
        node(),
        no_hooks.display()
    );
    let status = Command::new("cmd")
        .raw_arg(line)
        .current_dir(setup.project())
        .env("REFRAME_CLAUDE_HOME", setup.path("claude-home"))
        .env("REFRAME_FAKE_LOG", setup.path("fake.log"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .unwrap();
    assert!(status.success());
    let pid = wait_for("the outside claude to start", || {
        setup.fake_log().iter().rev().find_map(|entry| {
            (entry["event"] == "start" && args(entry).contains(&id.as_str()) && args(entry).contains(&flag))
                .then(|| entry["pid"].as_u64())
                .flatten()
        })
    });
    Outside { pid: pid as u32 }
}

/// Gone with the test, also one that failed: it would hold on to the test's output.
impl Drop for Outside {
    fn drop(&mut self) {
        self.kill();
    }
}

impl Outside {
    fn ended(&self, setup: &Setup) -> bool {
        setup
            .fake_log()
            .iter()
            .any(|entry| entry["event"] == "exit" && entry["pid"].as_u64() == Some(u64::from(self.pid)))
    }

    fn kill(&self) {
        let _ = Command::new("taskkill")
            .args(["/PID", &self.pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn running_count(setup: &Setup) -> usize {
    let mut running = 0i32;
    for entry in setup.fake_log() {
        match entry["event"].as_str() {
            Some("start") => running += 1,
            Some("exit") => running -= 1,
            _ => {}
        }
    }
    usize::try_from(running).unwrap_or(0)
}

#[test]
fn a_session_resumed_in_another_terminal_is_taken_over_not_run_twice() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    app.write(&id, "hello\r");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    app.cmd(json!({ "cmd": "end", "session": id }));
    wait_for("it to end", || (running_count(&setup) == 0).then_some(()));

    let outside = run_outside(&setup, Some(&claude_id));
    wait_for("the outside one to run", || (running_count(&setup) == 1).then_some(()));
    app.cmd(json!({ "cmd": "sidebar" }));
    // In reframe's part as running elsewhere, and not a second time among the other terminals'.
    let list = list_with(&mut app, "it running elsewhere", |list| {
        list["app"][0]["elsewhere"] == true
    });
    assert_eq!(list["running"], json!([]));

    app.cmd(json!({ "cmd": "row_click", "row": format!("app:{id}") }));
    let question = wait_for("the question", || {
        let question = app.cmd(json!({ "cmd": "window" }))["question"].clone();
        (!question.is_null()).then_some(question)
    });
    assert_eq!(question["message"], "alpha is running in another terminal.");
    app.cmd(json!({ "cmd": "answer", "yes": true }));
    wait_for("the outside one to end", || outside.ended(&setup).then_some(()));
    app.wait_status(&id, "idle");
    // Started here, resumed outside, taken over here: never twice at once.
    assert_eq!(running_count(&setup), 1);
    assert_eq!(starts_of(&setup, &claude_id), 3);
    app.quit();
}

#[test]
fn a_session_resumed_elsewhere_before_reframe_starts_waits_for_take_over() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "beta" }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    app.write(&id, "hello\r");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    app.quit();

    let outside = run_outside(&setup, Some(&claude_id));
    wait_for("the outside one to run", || (running_count(&setup) == 1).then_some(()));
    let mut app = setup.launch();
    // Nobody to ask on the start: the tile says why and waits.
    wait_for("the tile to say so", || {
        let sessions = app.cmd(json!({ "cmd": "sessions" }));
        (sessions[&id]["error"] == "runningElsewhere").then_some(())
    });
    assert_eq!(app.cmd(json!({ "cmd": "window" }))["question"], Value::Null);

    // "Take over": asked, then ended there and continued here.
    app.cmd(json!({ "cmd": "restart", "session": id }));
    wait_for("the question", || {
        (!app.cmd(json!({ "cmd": "window" }))["question"].is_null()).then_some(())
    });
    // No: it keeps waiting.
    app.cmd(json!({ "cmd": "answer", "yes": false }));
    wait_for("it to wait again", || {
        (app.cmd(json!({ "cmd": "sessions" }))[&id]["error"] == "runningElsewhere").then_some(())
    });
    assert!(!outside.ended(&setup));
    app.cmd(json!({ "cmd": "restart", "session": id }));
    wait_for("the question", || {
        (!app.cmd(json!({ "cmd": "window" }))["question"].is_null()).then_some(())
    });
    app.cmd(json!({ "cmd": "answer", "yes": true }));
    wait_for("the outside one to end", || outside.ended(&setup).then_some(()));
    app.wait_status(&id, "idle");
    app.quit();
}

#[test]
fn workspaces_save_a_window_open_it_again_rename_and_delete_it() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let first = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "one" }));
    let one = first["session"].as_str().unwrap().to_owned();
    let second = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "two" }));
    let two = second["session"].as_str().unwrap().to_owned();
    app.wait_status(&one, "idle");
    app.wait_status(&two, "idle");

    app.cmd(json!({ "cmd": "workspace", "action": "save", "name": "  Shop  " }));
    let state = app.state();
    let workspace = state["workspaces"][0].clone();
    assert_eq!(workspace["name"], "Shop");
    assert_eq!(workspace["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(state["windows"][0]["workspaceId"], workspace["id"]);
    let workspace_id = workspace["id"].as_str().unwrap().to_owned();
    // The toolbar's menu offers to update it, and lists it as open.
    let menu = app.cmd(json!({ "cmd": "workspace_menu" }))["menu"].clone();
    let labels: Vec<&str> = menu
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["label"].as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Save this window as a workspace …",
            "Update workspace \"Shop\"",
            "Shop (open)"
        ]
    );
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));

    // The tray offers it too.
    let tray = app.cmd(json!({ "cmd": "tray" }));
    let submenu = tray["menu"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["label"] == "Workspaces")
        .cloned()
        .unwrap();
    assert_eq!(submenu["children"][0]["label"], "Shop (open)");

    // Both sessions are ended; opened again, it is a window of its own with both of them back.
    app.cmd(json!({ "cmd": "end", "session": one }));
    app.cmd(json!({ "cmd": "end", "session": two }));
    wait_for("both to end", || (running_count(&setup) == 0).then_some(()));
    assert_eq!(app.state()["windows"][0]["layout"], Value::Null);
    app.cmd(json!({ "cmd": "workspace", "action": "open", "id": workspace_id }));
    let state = wait_for("its window", || {
        let state = app.state();
        (state["windows"].as_array().unwrap().len() == 2).then_some(state)
    });
    let window = state["windows"][1].clone();
    assert_eq!(window["workspaceId"], workspace_id);
    let layout = window["layout"].to_string();
    assert!(layout.contains(&one) && layout.contains(&two), "{layout}");
    app.wait_status(&one, "idle");
    app.wait_status(&two, "idle");
    // Opened again: the same window comes up, no second one.
    app.cmd(json!({ "cmd": "workspace", "action": "open", "id": workspace_id }));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(app.state()["windows"].as_array().unwrap().len(), 2);

    app.cmd(json!({ "cmd": "workspace", "action": "rename", "id": workspace_id, "name": "Store" }));
    assert_eq!(app.state()["workspaces"][0]["name"], "Store");

    // Deleting asks; no keeps it, yes forgets it, and its window stays as a plain window.
    let window_id = window["id"].as_str().unwrap().to_owned();
    app.cmd(json!({ "cmd": "workspace", "action": "delete", "id": workspace_id, "window": window_id }));
    let question = wait_for("the question", || {
        let question = app.cmd(json!({ "cmd": "window", "window": window_id }))["question"].clone();
        (!question.is_null()).then_some(question)
    });
    assert_eq!(question["message"], "Delete the workspace \"Store\"?");
    app.cmd(json!({ "cmd": "answer", "window": window_id, "yes": false }));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(app.state()["workspaces"].as_array().unwrap().len(), 1);
    app.cmd(json!({ "cmd": "workspace", "action": "delete", "id": workspace_id, "window": window_id }));
    wait_for("the question", || {
        (!app.cmd(json!({ "cmd": "window", "window": window_id }))["question"].is_null()).then_some(())
    });
    app.cmd(json!({ "cmd": "answer", "window": window_id, "yes": true }));
    let state = wait_for("it to be gone", || {
        let state = app.state();
        (state["workspaces"] == json!([])).then_some(state)
    });
    assert_eq!(state["windows"].as_array().unwrap().len(), 2);
    assert!(state["windows"][1].get("workspaceId").is_none());
    app.quit();
}

#[test]
fn the_list_shows_when_sessions_were_last_used_and_can_drop_them_into_the_layout() {
    let setup = Setup::new();
    let older = past(&setup, "Older task", 120);
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "here" }));
    let here = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&here, "idle");
    app.cmd(json!({ "cmd": "sidebar" }));
    let list = list_with(&mut app, "both", |list| {
        list["app"].as_array().is_some_and(|app| app.len() == 1)
            && list["found"].as_array().is_some_and(|f| f.len() == 1)
    });
    assert_eq!(list["app"][0]["windowNumber"], 1);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64;
    let found_at = list["found"][0]["modifiedAt"].as_f64().unwrap();
    assert!((now - found_at - 120.0 * 60_000.0).abs() < 60_000.0);

    // Dropped onto the right half of the tile: it opens next to it.
    let placement = app.cmd(json!({ "cmd": "placement" }));
    let tile = placement["tiles"][0]["rect"].as_array().unwrap().clone();
    let (x, y, w, h) = (
        tile[0].as_f64().unwrap(),
        tile[1].as_f64().unwrap(),
        tile[2].as_f64().unwrap(),
        tile[3].as_f64().unwrap(),
    );
    let dropped = app.cmd(json!({
        "cmd": "row_drop",
        "rows": [format!("claude:{older}")],
        "x": x + w - 10.0,
        "y": y + h / 2.0,
    }));
    assert_eq!(dropped["result"], "opened");
    let layout = wait_for("it to land", || {
        let layout = app.state()["windows"][0]["layout"].clone();
        (layout["kind"] == "split").then_some(layout)
    });
    assert_eq!(layout["dir"], "row");
    assert_eq!(layout["children"][0]["sessionId"], here);
    let opened = record_of(&mut app, &older);
    assert_eq!(layout["children"][1]["sessionId"], opened);
    // Opening it was not using it: its last use is still its transcript's.
    let record = app.record(&opened);
    assert!(
        (record["lastActiveAt"].as_f64().unwrap() - found_at).abs() < 1000.0,
        "{record}"
    );
    assert_eq!(record["used"], true);

    // A collapsed session dragged out of the list lands where it is dropped.
    app.cmd(json!({ "cmd": "layout", "action": "collapse", "session": here }));
    let tile = wait_for("the other tile to fill the layout", || {
        let placement = app.cmd(json!({ "cmd": "placement" }));
        let tiles = placement["tiles"].as_array().cloned().unwrap_or_default();
        (tiles.len() == 1 && tiles[0]["session"] == opened.as_str()).then(|| tiles[0]["rect"].clone())
    });
    let (x, y, h) = (
        tile[0].as_f64().unwrap(),
        tile[1].as_f64().unwrap(),
        tile[3].as_f64().unwrap(),
    );
    let dropped = app.cmd(json!({
        "cmd": "row_drop",
        "rows": [format!("app:{here}")],
        "x": x + 10.0,
        "y": y + h / 2.0,
    }));
    assert_eq!(dropped["result"], "moved");
    let state = wait_for("it to land at the left", || {
        let state = app.state();
        (state["windows"][0]["layout"]["kind"] == "split").then_some(state)
    });
    assert_eq!(state["windows"][0]["layout"]["children"][0]["sessionId"], here);
    assert_eq!(state["windows"][0]["dock"], json!([]));
    assert_eq!(app.status(&here), "idle", "moved, not restarted");

    // One that is in no window can be taken off the list.
    app.cmd(json!({ "cmd": "end", "session": opened }));
    app.cmd(json!({ "cmd": "remove", "session": opened }));
    // Claude Code's transcript is left alone: it is found again (once the list has looked again,
    // which takes a moment longer on a slow machine).
    list_with(&mut app, "it gone from reframe's part and found again", |list| {
        list["app"].as_array().is_some_and(|app| app.len() == 1) && titles(list, "found") == ["Older task"]
    });
    app.quit();
}
