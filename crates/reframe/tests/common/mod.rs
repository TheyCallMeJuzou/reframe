//! The app with the fake claude in a throwaway profile, driven over its test channel. Needs
//! `node` on the PATH (the fake claude is a script).
#![allow(dead_code)] // each test file uses its share

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub const WAIT: Duration = Duration::from_secs(20);

pub fn node() -> String {
    let found = Command::new("where").arg("node").output().expect("where node");
    let text = String::from_utf8_lossy(&found.stdout);
    text.lines().next().expect("node is not on the PATH").trim().to_owned()
}

pub fn wait_for<T>(what: &str, mut look: impl FnMut() -> Option<T>) -> T {
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
pub struct Setup {
    dir: tempfile::TempDir,
}

impl Setup {
    pub fn new() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("project")).unwrap();
        Setup { dir }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn project(&self) -> String {
        self.path("project").to_string_lossy().into_owned()
    }

    pub fn command(&self) -> Command {
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

    pub fn launch(&self) -> App {
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
    pub fn fake_log(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.path("fake.log")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub fn saved_state(&self) -> Value {
        let text = std::fs::read_to_string(self.path("user-data").join("state.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

pub struct App {
    pub child: Child,
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl App {
    pub fn cmd(&mut self, command: Value) -> Value {
        writeln!(self.stream, "{command}").unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("no answer to {command}: {line:?}"))
    }

    pub fn state(&mut self) -> Value {
        self.cmd(json!({ "cmd": "state" }))
    }

    pub fn record(&mut self, session: &str) -> Value {
        self.state()["sessions"][session].clone()
    }

    pub fn status(&mut self, session: &str) -> Value {
        self.cmd(json!({ "cmd": "sessions" }))[session]["status"].clone()
    }

    pub fn wait_status(&mut self, session: &str, status: &str) {
        wait_for(&format!("status {status}"), || {
            (self.status(session) == status).then_some(())
        });
    }

    pub fn write(&mut self, session: &str, text: &str) {
        assert_eq!(
            self.cmd(json!({ "cmd": "write", "session": session, "text": text }))["ok"],
            true
        );
    }

    pub fn screen(&mut self, session: &str) -> String {
        let answer = self.cmd(json!({ "cmd": "screen", "session": session }));
        answer["text"].as_str().unwrap_or_default().to_owned()
    }

    pub fn wait_screen(&mut self, session: &str, text: &str) {
        wait_for(&format!("{text:?} on screen"), || {
            self.screen(session).contains(text).then_some(())
        });
    }

    /// Quits the way the user would and waits for the process to be gone.
    pub fn quit(mut self) {
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

pub fn args(entry: &Value) -> Vec<&str> {
    entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}
