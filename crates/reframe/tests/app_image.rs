//! The AppImage updates itself on Linux, as 0.x's did through electron-updater: a release feed
//! served by the test stands in for GitHub (`latest-linux.yml`), a script for the new AppImage.
//! On quit it takes the place of the running one (under its own name when the old name carries a
//! version); "Restart to update" also starts it.
#![cfg(target_os = "linux")]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use common::feed::{Feed, sha512_of};
use common::{App, Setup, wait_for};
use serde_json::json;

/// The new "AppImage": a script that notes how it was started.
fn new_app_image(marker: &std::path::Path) -> Vec<u8> {
    format!(
        "#!/bin/sh\necho \"silent=$APPIMAGE_SILENT_INSTALL\" > '{}'\n",
        marker.display()
    )
    .into_bytes()
}

/// The running AppImage (as `APPIMAGE` names it), in a folder of its own.
fn old_app_image(setup: &Setup) -> PathBuf {
    let folder = setup.path("Apps");
    std::fs::create_dir_all(&folder).unwrap();
    let old = folder.join("reframe-1.0.0-x86_64.AppImage");
    std::fs::write(&old, b"the running AppImage").unwrap();
    old
}

fn launch(setup: &Setup, feed: &Feed, running: &std::path::Path) -> App {
    let mut command = setup.command();
    command.env("REFRAME_UPDATE_FEED", &feed.url).env("APPIMAGE", running);
    setup.launch_command(command, &[])
}

fn download(app: &mut App, version: &str) {
    let report = app.cmd(json!({ "cmd": "updates", "mode": "install", "check": true }));
    assert_eq!(report["checks"], true);
    wait_for("the download", || {
        (app.cmd(json!({ "cmd": "updates" }))["ready"] == version).then_some(())
    });
}

#[test]
fn the_app_image_downloads_a_newer_one_and_puts_it_in_its_place_on_quit() {
    let setup = Setup::new();
    let marker = setup.path("started");
    let bytes = new_app_image(&marker);
    let feed = Feed::start(
        "9.0.0",
        "latest-linux.yml",
        "reframe-9.0.0-x86_64.AppImage",
        &bytes,
        &sha512_of(&bytes),
    );
    let old = old_app_image(&setup);
    let mut app = launch(&setup, &feed, &old);
    download(&mut app, "9.0.0");
    assert!(
        feed.asked()
            .iter()
            .any(|path| path == "/download/v9.0.0/latest-linux.yml")
    );
    // Nothing changes before reframe quits.
    assert!(old.exists());

    app.quit();
    let placed = old.with_file_name("reframe-9.0.0-x86_64.AppImage");
    assert!(!old.exists(), "the old AppImage is gone");
    assert_eq!(std::fs::read(&placed).unwrap(), bytes);
    let mode = std::fs::metadata(&placed).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    // Installed on quit, not started.
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!marker.exists());
    assert!(setup.log().contains("update installed"), "{}", setup.log());
}

#[test]
fn restart_to_update_starts_the_new_app_image_and_a_chosen_name_stays() {
    let setup = Setup::new();
    let marker = setup.path("started");
    let bytes = new_app_image(&marker);
    let feed = Feed::start(
        "9.0.0",
        "latest-linux.yml",
        "reframe-9.0.0-x86_64.AppImage",
        &bytes,
        &sha512_of(&bytes),
    );
    // A name the user chose (no version in it) stays.
    let folder = setup.path("Apps");
    std::fs::create_dir_all(&folder).unwrap();
    let old = folder.join("reframe.AppImage");
    std::fs::write(&old, b"the running AppImage").unwrap();
    let mut app = launch(&setup, &feed, &old);
    download(&mut app, "9.0.0");

    assert_eq!(
        app.cmd(json!({ "cmd": "tray_click", "label": "Restart to update to 9.0.0" }))["ok"],
        true
    );
    let child = &mut app.child;
    let status = wait_for("reframe to quit", || child.try_wait().unwrap());
    assert!(status.success(), "quit with {status}");
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    let started = wait_for("the new AppImage to start", || std::fs::read_to_string(&marker).ok());
    assert_eq!(started.trim(), "silent=true");
}

#[test]
fn a_download_that_does_not_match_is_not_installed() {
    let setup = Setup::new();
    let marker = setup.path("started");
    let bytes = new_app_image(&marker);
    let feed = Feed::start(
        "9.0.0",
        "latest-linux.yml",
        "reframe-9.0.0-x86_64.AppImage",
        &bytes,
        &sha512_of(b"something else"),
    );
    let old = old_app_image(&setup);
    let mut app = launch(&setup, &feed, &old);
    app.cmd(json!({ "cmd": "updates", "mode": "install", "check": true }));
    wait_for("the check to fail", || {
        setup.log().contains("does not match latest-linux.yml").then_some(())
    });
    assert!(app.cmd(json!({ "cmd": "updates" }))["ready"].is_null());
    app.quit();
    assert_eq!(std::fs::read(&old).unwrap(), b"the running AppImage");
}
