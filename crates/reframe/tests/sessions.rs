//! The app with the fake claude in a throwaway profile, driven over its test channel: what the
//! Electron version's `e2e/sessions.spec.ts`, `resume.spec.ts`, `start-failure.spec.ts` and
//! `state-file.spec.ts` checked about sessions. Needs `node` on the PATH (the fake claude is a
//! script).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(20);

fn node() -> String {
    let found = Command::new("where").arg("node").output().expect("where node");
    let text = String::from_utf8_lossy(&found.stdout);
    text.lines().next().expect("node is not on the PATH").trim().to_owned()
}

fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(found) = look() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A profile, a project folder and a Claude Code folder of their own.
struct Setup {
    dir: tempfile::TempDir,
}

impl Setup {
    fn new() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("project")).unwrap();
        Setup { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn project(&self) -> String {
        self.path("project").to_string_lossy().into_owned()
    }

    fn command(&self) -> Command {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let fake = root.join("test").join("fake-claude").join("fake-claude.mjs");
        let mut command = Command::new(env!("CARGO_BIN_EXE_reframe-native"));
        command
            .env("REFRAME_E2E", "1")
            .env("REFRAME_USER_DATA", self.path("user-data"))
            .env("REFRAME_CLAUDE_HOME", self.path("claude-home"))
            .env(
                "REFRAME_CLAUDE_BIN",
                fake.canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\"),
            )
            .env("REFRAME_NODE", node())
            .env("REFRAME_FAKE_LOG", self.path("fake.log"))
            .env("REFRAME_LOCALE", "en")
            .env("REFRAME_DISABLE_NOTIFICATIONS", "1")
            .stdin(Stdio::null());
        command
    }

    fn launch(&self) -> App {
        let port_file = self.path("user-data").join("e2e-port");
        let _ = std::fs::remove_file(&port_file);
        let child = self.command().spawn().expect("the app did not start");
        let port: u16 = wait_for("the app to be ready", || {
            std::fs::read_to_string(&port_file).ok()?.parse().ok()
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        App {
            child,
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
        }
    }

    /// What the fake claudes logged: one entry per start and exit.
    fn fake_log(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.path("fake.log")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn saved_state(&self) -> Value {
        let text = std::fs::read_to_string(self.path("user-data").join("state.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

struct App {
    child: Child,
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl App {
    fn cmd(&mut self, command: Value) -> Value {
        writeln!(self.stream, "{command}").unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("no answer to {command}: {line:?}"))
    }

    fn state(&mut self) -> Value {
        self.cmd(json!({ "cmd": "state" }))
    }

    fn record(&mut self, session: &str) -> Value {
        self.state()["sessions"][session].clone()
    }

    fn status(&mut self, session: &str) -> Value {
        self.cmd(json!({ "cmd": "sessions" }))[session]["status"].clone()
    }

    fn wait_status(&mut self, session: &str, status: &str) {
        wait_for(&format!("status {status}"), || {
            (self.status(session) == status).then_some(())
        });
    }

    fn write(&mut self, session: &str, text: &str) {
        assert_eq!(
            self.cmd(json!({ "cmd": "write", "session": session, "text": text }))["ok"],
            true
        );
    }

    fn screen(&mut self, session: &str) -> String {
        let answer = self.cmd(json!({ "cmd": "screen", "session": session }));
        answer["text"].as_str().unwrap_or_default().to_owned()
    }

    fn wait_screen(&mut self, session: &str, text: &str) {
        wait_for(&format!("{text:?} on screen"), || {
            self.screen(session).contains(text).then_some(())
        });
    }

    /// Quits the way the user would and waits for the process to be gone.
    fn quit(mut self) {
        self.cmd(json!({ "cmd": "quit" }));
        let child = &mut self.child;
        let status = wait_for("the app to quit", || child.try_wait().unwrap());
        assert!(status.success(), "quit with {status}");
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn args(entry: &Value) -> Vec<&str> {
    entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn a_session_through_its_life() {
    let setup = Setup::new();
    let mut app = setup.launch();

    // A folder that is not there gets no session.
    let missing = app.cmd(json!({ "cmd": "new", "cwd": setup.path("nowhere") }));
    assert_eq!(missing, json!({ "ok": false, "error": "folderMissing" }));
    assert_eq!(app.state()["sessions"], json!({}));

    // A new session starts claude under the session id reframe chose, with hook settings.
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "shop" }));
    assert_eq!(created["ok"], true, "{created}");
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    app.wait_screen(&id, "FAKE CLAUDE new");
    let record = app.record(&id);
    let first_claude_id = record["claudeSessionId"].as_str().unwrap().to_owned();
    assert_eq!(record["lifecycle"], "open");
    assert_eq!(record["name"], "shop");
    assert!(record["pid"].is_u64());
    assert!(record.get("used").is_none(), "{record}");
    let state = app.state();
    assert_eq!(
        state["windows"][0]["layout"],
        json!({ "kind": "tile", "sessionId": id })
    );
    assert_eq!(state["recentFolders"], json!([setup.project()]));
    let log = setup.fake_log();
    let started = args(&log[0]);
    assert_eq!(started[..2], ["--session-id", first_claude_id.as_str()]);
    assert!(started.contains(&"--settings") && started.contains(&"--name") && started.contains(&"shop"));

    // A prompt: busy, then idle, and the session counts as used.
    let before = record["lastActiveAt"].as_f64().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    app.write(&id, "hello\r");
    app.wait_screen(&id, "echo: hello");
    app.wait_status(&id, "idle");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    assert!(app.record(&id)["lastActiveAt"].as_f64().unwrap() > before);

    // Claude asks: the session waits for the user until the answer.
    app.write(&id, "/ask\r");
    app.wait_status(&id, "waiting");
    app.write(&id, "\r");
    app.wait_status(&id, "idle");

    // /clear: a new claude session id (seen in the registry, there is no hook), unused again.
    app.write(&id, "/clear\r");
    let cleared = wait_for("the new claude session id", || {
        let now = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
        (now != first_claude_id).then_some(now)
    });
    assert_eq!(app.record(&id)["used"], false);
    app.write(&id, "again\r");
    wait_for("the cleared session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    app.wait_status(&id, "idle");

    // Quitting ends claude; the session stays open and its conversation is resumed next time.
    app.quit();
    let log = setup.fake_log();
    assert_eq!(log.iter().filter(|entry| entry["event"] == "exit").count(), 1);
    let saved = setup.saved_state();
    assert_eq!(saved["sessions"][&id]["lifecycle"], "open");
    assert_eq!(saved["sessions"][&id]["claudeSessionId"], cleared.as_str());

    let mut app = setup.launch();
    app.wait_status(&id, "idle");
    app.wait_screen(&id, &format!("FAKE CLAUDE resumed {cleared}"));
    let log = setup.fake_log();
    assert_eq!(args(log.last().unwrap())[..2], ["--resume", cleared.as_str()]);

    // Claude exits by itself (Ctrl+C twice): the session is ended and stays in its tile.
    app.write(&id, "\x03");
    app.write(&id, "\x03");
    app.wait_status(&id, "ended");
    wait_for("the session to be ended", || {
        (app.record(&id)["lifecycle"] == "ended").then_some(())
    });
    assert_eq!(app.state()["windows"][0]["layout"]["sessionId"], id.as_str());

    // Started again, it resumes its conversation.
    app.cmd(json!({ "cmd": "restart", "session": id }));
    app.wait_status(&id, "idle");
    assert_eq!(app.record(&id)["lifecycle"], "open");
    assert_eq!(
        args(setup.fake_log().last().unwrap())[..2],
        ["--resume", cleared.as_str()]
    );

    // Started fresh, it is a new conversation in the same tile.
    app.write(&id, "\x03");
    app.write(&id, "\x03");
    app.wait_status(&id, "ended");
    app.cmd(json!({ "cmd": "restart", "session": id, "fresh": true }));
    app.wait_status(&id, "idle");
    let fresh = app.record(&id);
    let fresh_id = fresh["claudeSessionId"].as_str().unwrap();
    assert_ne!(fresh_id, cleared);
    assert_eq!(fresh["used"], false);
    assert_eq!(args(setup.fake_log().last().unwrap())[..2], ["--session-id", fresh_id]);

    // Ended by the user: claude is ended, the tile goes, the session stays in the list.
    let running = app.record(&id)["pid"].as_u64().unwrap();
    app.cmd(json!({ "cmd": "end", "session": id }));
    assert_eq!(app.state()["windows"][0]["layout"], Value::Null);
    assert_eq!(app.record(&id)["lifecycle"], "ended");
    wait_for("claude to exit", || {
        let log = setup.fake_log();
        let last = log.last()?;
        (last["event"] == "exit" && last["pid"] == running).then_some(())
    });
    wait_for("the session's process to be forgotten", || {
        app.status(&id).is_null().then_some(())
    });

    // Removed from the list, it is gone, with its hook settings.
    let hook_settings = setup.path("user-data").join("hooks").join(format!("{id}.json"));
    assert!(hook_settings.is_file());
    app.cmd(json!({ "cmd": "remove", "session": id }));
    assert_eq!(app.state()["sessions"], json!({}));
    assert!(!hook_settings.exists());
    app.quit();
    assert_eq!(setup.saved_state()["sessions"], json!({}));
}

#[test]
fn a_used_session_without_its_transcript_says_so() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project() }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    app.write(&id, "hello\r");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    app.wait_status(&id, "idle");
    app.quit();

    // Its conversation is gone: nothing to resume, and starting fresh would lose what it was.
    std::fs::remove_dir_all(setup.path("claude-home").join("projects")).unwrap();
    let starts = setup.fake_log().len();
    let mut app = setup.launch();
    wait_for("the error", || {
        let sessions = app.cmd(json!({ "cmd": "sessions" }));
        (sessions[&id]["error"] == "transcriptMissing").then_some(())
    });
    assert_eq!(app.status(&id), "error");
    assert_eq!(setup.fake_log().len(), starts, "claude was started");
    app.quit();
}

#[test]
fn sessions_resume_after_a_crash() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project() }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    app.write(&id, "hello\r");
    wait_for("the session to count as used", || {
        (app.record(&id)["used"] == true).then_some(())
    });
    app.wait_status(&id, "idle");
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    // Gone without a word: nothing is ended, nothing is saved.
    app.child.kill().unwrap();
    app.child.wait().unwrap();
    drop(app);

    let mut app = setup.launch();
    app.wait_status(&id, "idle");
    app.wait_screen(&id, &format!("FAKE CLAUDE resumed {claude_id}"));
    let log = setup.fake_log();
    let starts: Vec<&Value> = log.iter().filter(|entry| entry["event"] == "start").collect();
    assert_eq!(starts.len(), 2, "{log:?}");
    assert_eq!(args(starts[1])[..2], ["--resume", claude_id.as_str()]);
    // What the first one left in the registry under its process id is not taken for the new one.
    app.write(&id, "again\r");
    app.wait_screen(&id, "echo: again");
    app.wait_status(&id, "idle");
    assert_eq!(app.record(&id)["claudeSessionId"], claude_id.as_str());
    app.quit();
}

#[test]
fn an_unused_session_starts_fresh_under_the_same_id() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project() }));
    let id = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&id, "idle");
    let claude_id = app.record(&id)["claudeSessionId"].as_str().unwrap().to_owned();
    app.quit();

    let mut app = setup.launch();
    app.wait_status(&id, "idle");
    assert_eq!(
        args(setup.fake_log().last().unwrap())[..2],
        ["--session-id", claude_id.as_str()]
    );
    app.quit();
}

#[test]
fn a_state_file_of_a_newer_reframe_is_left_alone() {
    let setup = Setup::new();
    let file = setup.path("user-data").join("state.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let newer = r#"{"version":2,"sessions":{},"windows":[],"somethingNew":true}"#;
    std::fs::write(&file, newer).unwrap();
    let mut child = setup.command().stderr(Stdio::null()).spawn().unwrap();
    let status = wait_for("the app to give up", || child.try_wait().unwrap());
    assert_eq!(status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
    let log = std::fs::read_to_string(setup.path("user-data").join("logs").join("main.log")).unwrap();
    assert!(log.contains("state file is from a newer reframe"), "{log}");
}
