//! Updates: a release feed served by the test stands in for GitHub, and a script stands in for the
//! installer. What the Electron version left to electron-updater and checked in
//! `src/main/updates.test.ts` and `e2e/settings.spec.ts`: an installed copy downloads a newer
//! version, checks it and installs it on quit or with "Restart to update"; a download that does
//! not match is thrown away; a portable copy only tells; another build started says so.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use common::{App, Setup, wait_for};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};

const INSTALLER: &[u8] = b"not really an installer, but bytes to download and check";

/// A release feed as GitHub serves it, on a port of its own: `/latest` (the newest tag),
/// `/download/<tag>/latest.yml` and the installer. Remembers what was asked for.
struct Feed {
    url: String,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Feed {
    fn start(version: &str, sha512: &str) -> Feed {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let tag = format!("v{version}");
        let file = format!("reframe-Setup-{version}.exe");
        let yml = format!(
            "version: {version}\nfiles:\n  - url: {file}\n    sha512: {sha512}\n    size: {}\npath: {file}\nsha512: {sha512}\nreleaseDate: '2026-11-20T10:00:00.000Z'\n",
            INSTALLER.len()
        );
        let routes: Vec<(String, Vec<u8>)> = vec![
            (
                "/latest".to_owned(),
                json!({ "tag_name": tag }).to_string().into_bytes(),
            ),
            (format!("/download/{tag}/latest.yml"), yml.into_bytes()),
            (format!("/download/{tag}/{file}"), INSTALLER.to_vec()),
        ];
        let log = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // The rest of the request.
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
                log.lock().unwrap().push(path.clone());
                let mut stream = stream;
                match routes.iter().find(|(route, _)| *route == path) {
                    Some((_, body)) => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(body);
                    }
                    None => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                    }
                }
            }
        });
        Feed { url, asked }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

fn sha512_of(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
}

fn fake_installer() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("fake-installer")
        .join("fake-installer.mjs")
        .canonicalize()
        .unwrap()
}

fn plain(path: &Path) -> String {
    path.to_string_lossy().trim_start_matches(r"\\?\").to_owned()
}

fn launch(setup: &Setup, feed: &Feed) -> App {
    let mut command = setup.command();
    command
        .env("REFRAME_UPDATE_FEED", &feed.url)
        .env("REFRAME_FAKE_INSTALLER", plain(&fake_installer()))
        .env("REFRAME_FAKE_INSTALLER_LOG", setup.path("installer.log"));
    setup.launch_command(command, &[])
}

/// What the stand-in installer was started with.
fn installer_runs(setup: &Setup) -> Vec<Value> {
    std::fs::read_to_string(setup.path("installer.log"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn tray_labels(app: &mut App) -> Vec<String> {
    app.cmd(json!({ "cmd": "tray" }))["menu"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["label"].as_str().map(str::to_owned))
        .collect()
}

fn quit_and_wait(mut app: App) {
    app.cmd(json!({ "cmd": "quit" }));
    let status = wait_for("the app to quit", || app.child.try_wait().unwrap());
    assert!(status.success(), "quit with {status}");
}

#[test]
fn an_installed_copy_downloads_a_newer_version_and_installs_it_on_quit() {
    let setup = Setup::new();
    let feed = Feed::start("9.0.0", &sha512_of(INSTALLER));
    let mut app = launch(&setup, &feed);
    // A development build does not check; Settings has no checkbox for it.
    assert_eq!(app.cmd(json!({ "cmd": "updates" }))["checks"], false);
    let report = app.cmd(json!({ "cmd": "updates", "mode": "install", "check": true }));
    assert_eq!(report["checks"], true);
    wait_for("the download", || {
        (app.cmd(json!({ "cmd": "updates" }))["ready"] == "9.0.0").then_some(())
    });
    assert!(tray_labels(&mut app).contains(&"Restart to update to 9.0.0".to_owned()));
    let downloaded = setup
        .path("user-data")
        .join("reframe-updater")
        .join("pending")
        .join("reframe-Setup-9.0.0.exe");
    assert_eq!(std::fs::read(&downloaded).unwrap(), INSTALLER);
    // Checked again: it is not downloaded a second time.
    app.cmd(json!({ "cmd": "updates", "check": true }));
    wait_for("the second check", || {
        (feed.asked().iter().filter(|path| path.ends_with("latest.yml")).count() == 2).then_some(())
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    let downloads = feed.asked().iter().filter(|path| path.ends_with(".exe")).count();
    assert_eq!(downloads, 1, "{:?}", feed.asked());
    assert!(
        installer_runs(&setup).is_empty(),
        "nothing is installed before reframe quits"
    );

    quit_and_wait(app);
    let runs = wait_for("the installer", || {
        let runs = installer_runs(&setup);
        (!runs.is_empty()).then_some(runs)
    });
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["args"], json!(["--updated", "/S"]));
    assert_eq!(runs[0]["bytes"], INSTALLER.len());
    assert!(
        Path::new(runs[0]["installer"].as_str().unwrap()).ends_with("reframe-Setup-9.0.0.exe"),
        "{runs:?}"
    );
    let log = setup.log();
    assert!(log.contains("update downloaded, installs on the next quit"), "{log}");
    assert!(log.contains("installing the update"), "{log}");
}

#[test]
fn restart_to_update_ends_the_sessions_and_the_installer_starts_reframe_again() {
    let setup = Setup::new();
    let feed = Feed::start("9.0.0", &sha512_of(INSTALLER));
    let mut app = launch(&setup, &feed);
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let alpha = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&alpha, "idle");
    app.cmd(json!({ "cmd": "updates", "mode": "install", "check": true }));
    wait_for("the download", || {
        (app.cmd(json!({ "cmd": "updates" }))["ready"] == "9.0.0").then_some(())
    });
    app.cmd(json!({ "cmd": "tray_click", "label": "Restart to update to 9.0.0" }));
    let status = wait_for("the app to quit", || app.child.try_wait().unwrap());
    assert!(status.success(), "quit with {status}");
    let runs = wait_for("the installer", || {
        let runs = installer_runs(&setup);
        (!runs.is_empty()).then_some(runs)
    });
    assert_eq!(runs[0]["args"], json!(["--updated", "--force-run"]));
    // The session ended as on quit and resumes after the restart.
    assert!(setup.fake_log().iter().any(|entry| entry["event"] == "exit"));
    let saved = setup.saved_state();
    assert_ne!(saved["sessions"][&alpha]["lifecycle"], "ended", "{saved}");
    assert!(setup.log().contains("restarting to install an update"));
}

#[test]
fn a_download_that_does_not_match_its_checksum_is_thrown_away() {
    let setup = Setup::new();
    let feed = Feed::start("9.0.0", &sha512_of(b"something else"));
    let mut app = launch(&setup, &feed);
    app.cmd(json!({ "cmd": "updates", "mode": "install", "check": true }));
    wait_for("the check to fail", || {
        setup.log().contains("update check failed").then_some(())
    });
    assert!(setup.log().contains("SHA-512 does not match"), "{}", setup.log());
    assert_eq!(app.cmd(json!({ "cmd": "updates" }))["ready"], Value::Null);
    let pending = setup.path("user-data").join("reframe-updater").join("pending");
    let left: Vec<_> = std::fs::read_dir(&pending)
        .map(|dir| dir.flatten().collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    assert!(
        !tray_labels(&mut app)
            .iter()
            .any(|label| label.starts_with("Restart to update"))
    );
    quit_and_wait(app);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(installer_runs(&setup).is_empty());
}

#[test]
fn a_portable_copy_only_tells_about_a_newer_version() {
    let setup = Setup::new();
    let feed = Feed::start("9.0.0", &sha512_of(INSTALLER));
    let mut app = launch(&setup, &feed);
    app.cmd(json!({ "cmd": "updates", "mode": "notify", "check": true }));
    wait_for("the newer version", || {
        (app.cmd(json!({ "cmd": "updates" }))["available"] == "9.0.0").then_some(())
    });
    assert!(tray_labels(&mut app).contains(&"Download reframe 9.0.0".to_owned()));
    assert!(
        !feed.asked().iter().any(|path| path.ends_with(".exe")),
        "{:?}",
        feed.asked()
    );
    assert!(setup.log().contains("update available for the portable copy"));
    quit_and_wait(app);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(installer_runs(&setup).is_empty());
}

#[test]
fn an_older_or_equal_release_is_no_update() {
    let setup = Setup::new();
    let feed = Feed::start("0.0.1", &sha512_of(INSTALLER));
    let mut app = launch(&setup, &feed);
    app.cmd(json!({ "cmd": "updates", "mode": "install", "check": true }));
    wait_for("the check", || {
        setup.log().contains("update check: no newer version").then_some(())
    });
    assert_eq!(app.cmd(json!({ "cmd": "updates" }))["ready"], Value::Null);
    assert!(!feed.asked().iter().any(|path| path.ends_with(".exe")));
    app.quit();
}

#[test]
fn another_build_started_for_the_same_profile_is_told_which_one_runs() {
    let setup = Setup::new();
    let feed = Feed::start("0.0.1", &sha512_of(INSTALLER));
    let mut app = launch(&setup, &feed);
    // The same build again: the windows come up, no question.
    app.cmd(json!({ "cmd": "second_start", "build": 1 }));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(app.cmd(json!({ "cmd": "window" }))["question"], Value::Null);
    // The installed one, while this development build runs.
    app.cmd(json!({ "cmd": "second_start", "build": 2 }));
    let question = wait_for("the notice", || {
        let question = app.cmd(json!({ "cmd": "window" }))["question"].clone();
        (!question.is_null()).then_some(question)
    });
    assert_eq!(question["title"], "reframe is already running");
    assert_eq!(
        question["message"],
        "You started the installed version, but the development build (cargo run) is running already, and only one can run at a time."
    );
    assert_eq!(question["cancel"], Value::Null);
    app.quit();
}
