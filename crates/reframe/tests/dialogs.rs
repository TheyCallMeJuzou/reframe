//! The dialogs as a user (and a screen reader) meets them, through UI Automation: what the
//! Electron version's `e2e/about.spec.ts` and `e2e/dialog.spec.ts` checked.
// UI Automation finds and presses the dialogs' fields: on Windows (the dialogs are the same code on Linux).
#![cfg(windows)]

mod common;

use common::{Setup, wait_for};
use serde_json::{Value, json};

/// Replaces what the text field holds with `text`, as typing would.
fn fill(app: &mut common::App, ui: &common::ui::Ui, field: &str, text: &str) {
    ui.focus("edit", field);
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-a" }));
    app.cmd(json!({ "cmd": "key", "keys": "backspace" }));
    if !text.is_empty() {
        app.cmd(json!({ "cmd": "type", "text": text }));
    }
    ui.wait_value("edit", field, text);
}

fn dialog_open(app: &mut common::App) -> bool {
    app.cmd(json!({ "cmd": "window" }))["dialog"] == true
}

#[test]
fn about_shows_the_version_the_license_and_its_links() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let ui = app.ui();
    ui.click("button", "About reframe");
    ui.wait("text", &format!("Version {}", env!("CARGO_PKG_VERSION")));
    ui.wait("text", "Made with ❤️ in Austria by reframe Team aka");

    ui.click("button", "License (MIT)");
    let license = wait_for("the license", || ui.with_role("document").into_iter().next());
    assert!(license.name.contains("MIT License"), "{}", license.name);
    assert!(license.name.contains("reframe Team"), "{}", license.name);

    // Its links open in the browser (caught here).
    ui.click("link", "reframe on GitHub");
    ui.click("link", "TheyCallMeJuzou");
    let opened = wait_for("the links", || {
        let links = app.cmd(json!({ "cmd": "links" }));
        (links.as_array().map_or(0, Vec::len) == 2).then_some(links)
    });
    assert_eq!(
        opened,
        json!([
            "https://github.com/TheyCallMeJuzou/reframe",
            "https://github.com/TheyCallMeJuzou"
        ])
    );

    ui.click("button", "Close");
    wait_for("the dialog to close", || (!dialog_open(&mut app)).then_some(()));
    app.quit();
}

#[test]
fn the_session_name_follows_the_folder_and_the_choice_is_remembered() {
    let setup = Setup::new();
    let project = setup.project();
    let other = setup.path("other-project");
    std::fs::create_dir(&other).unwrap();
    let other = other.to_string_lossy().into_owned();
    let (folder, name, from_folder) = ("Folder", "Name (optional)", "Use the folder name");

    let mut app = setup.launch();
    let ui = app.ui();
    app.cmd(json!({ "cmd": "open", "dialog": "new" }));
    ui.wait_checked(from_folder, true);

    // The name follows the folder ...
    fill(&mut app, &ui, folder, &project);
    ui.wait_value("edit", name, "project");
    fill(&mut app, &ui, folder, &other);
    ui.wait_value("edit", name, "other-project");

    // ... until the user types one.
    fill(&mut app, &ui, name, "mine");
    fill(&mut app, &ui, folder, &project);
    ui.wait_value("edit", name, "mine");

    // Unchecking keeps a typed name; checking again follows the folder again.
    ui.click("checkbox", from_folder);
    ui.wait_checked(from_folder, false);
    ui.wait_value("edit", name, "mine");
    ui.click("checkbox", from_folder);
    ui.wait_value("edit", name, "project");

    // Turned off, it stays off in the next dialog and after a restart.
    ui.click("checkbox", from_folder);
    ui.wait_value("edit", name, "");
    ui.click("button", "Cancel");
    wait_for("the dialog to close", || (!dialog_open(&mut app)).then_some(()));
    app.cmd(json!({ "cmd": "open", "dialog": "new" }));
    ui.wait_checked(from_folder, false);
    fill(&mut app, &ui, folder, &project);
    ui.wait_value("edit", name, "");
    wait_for("the choice to be saved", || {
        (app.state()["preferences"]["nameFromFolder"] == false).then_some(())
    });
    app.quit();

    let mut app = setup.launch();
    let ui = app.ui();
    app.cmd(json!({ "cmd": "open", "dialog": "new" }));
    ui.wait_checked(from_folder, false);
    app.quit();
}

#[test]
fn without_claude_code_the_dialog_says_how_to_get_it() {
    let setup = Setup::new();
    // No claude anywhere: no stand-in, and a PATH without one.
    let mut command = setup.command();
    command.env("REFRAME_CLAUDE_BIN", "");
    let empty = setup.path("empty");
    std::fs::create_dir(&empty).unwrap();
    for (key, _) in std::env::vars() {
        if key.eq_ignore_ascii_case("PATH") {
            command.env(&key, &empty);
        }
    }
    let mut app = setup.launch_command(command, &[]);
    let ui = app.ui();
    app.cmd(json!({ "cmd": "open", "dialog": "new" }));
    fill(&mut app, &ui, "Folder", &setup.project());
    ui.click("button", "Start");
    let error = wait_for("the error", || {
        ui.with_role("text")
            .into_iter()
            .find(|seen| seen.name.contains("claude.com/claude-code"))
    });
    assert!(
        error
            .name
            .contains("Install it on this computer: claude.com/claude-code"),
        "{}",
        error.name
    );
    assert!(
        error.name.contains("Installed it only in WSL? Choose a folder in WSL"),
        "{}",
        error.name
    );
    assert_eq!(app.cmd(json!({ "cmd": "sessions" })), Value::Object(Default::default()));
    app.quit();
}
