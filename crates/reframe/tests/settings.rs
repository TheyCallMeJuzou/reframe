//! Settings (the gear in the toolbar, or Ctrl+,), through UI Automation: what the Electron
//! version's `e2e/settings.spec.ts` checked. The language applies at once and after a restart;
//! the text size has buttons too; update checks can be turned off where reframe checks; what
//! closing a window with sessions does can be chosen; starting with the system is turned on and
//! off here; snapping can be turned off; which claude runs and what new sessions start with.

mod common;

use common::{Setup, args, wait_for};
use serde_json::json;

fn preference(app: &mut common::App, name: &str) -> serde_json::Value {
    app.state()["preferences"][name].clone()
}

fn wait_preference(app: &mut common::App, name: &str, value: serde_json::Value) {
    wait_for(&format!("{name} to be {value}"), || {
        (preference(app, name) == value).then_some(())
    });
}

/// Picks `option` in a choice box with the keyboard, as a screen reader user would: open it,
/// go down to an entry, take it, until the box shows `option`.
fn choose(app: &mut common::App, ui: &common::ui::Ui, name: &str, option: &str) {
    // The list opens at the entry the box shows: the wanted one is below it or above it.
    let tries = std::iter::once(("down", 0)).chain((1..10).flat_map(|steps| [("down", steps), ("up", steps)]));
    for (key, steps) in tries {
        ui.focus("combobox", name);
        app.cmd(json!({ "cmd": "key", "keys": "enter" }));
        std::thread::sleep(std::time::Duration::from_millis(150));
        for _ in 0..steps {
            app.cmd(json!({ "cmd": "key", "keys": key }));
        }
        app.cmd(json!({ "cmd": "key", "keys": "enter" }));
        std::thread::sleep(std::time::Duration::from_millis(150));
        // Picking another language renames the box itself.
        let shown = ["Language", "Sprache", name]
            .into_iter()
            .filter(|box_name| *box_name == name || name == "Language" || name == "Sprache")
            .find_map(|box_name| ui.has("combobox", box_name).then(|| ui.wait("combobox", box_name)));
        if shown.and_then(|seen| seen.value).as_deref() == Some(option) {
            return;
        }
    }
    panic!("{name:?} does not offer {option:?}");
}

fn log(setup: &Setup) -> String {
    setup.log()
}

#[test]
fn language_text_size_update_checks_closing_windows_and_starting_with_the_system() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let ui = app.ui();
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-," }));
    ui.wait("combobox", "Language");

    // The text size by its buttons.
    ui.click("button", "+");
    wait_preference(&mut app, "fontSize", json!(14));
    ui.click("button", "Normal");
    wait_preference(&mut app, "fontSize", json!(13));

    // Update checks: offered only where this copy checks (in the tests it does not, until it is
    // told to).
    assert!(!ui.has("checkbox", "Check for updates"));
    app.cmd(json!({ "cmd": "updates", "mode": "install" }));
    ui.wait_checked("Check for updates", true);
    ui.click("checkbox", "Check for updates");
    wait_preference(&mut app, "checkForUpdates", json!(false));
    ui.click("checkbox", "Check for updates");
    wait_preference(&mut app, "checkForUpdates", json!(true));

    // Closing a window with sessions: asks, until something else is chosen.
    ui.wait_value("combobox", "Closing a window with sessions", "Ask");
    choose(&mut app, &ui, "Closing a window with sessions", "Ends them");
    wait_preference(&mut app, "closeWindow", json!("end"));
    choose(&mut app, &ui, "Closing a window with sessions", "Ask");
    wait_preference(&mut app, "closeWindow", json!("ask"));

    // Starting with the system (the tests leave the system's own setting alone): on, and still on
    // when Settings opens again.
    ui.wait_checked("Start with the system", false);
    ui.click("checkbox", "Start with the system");
    wait_for("the log", || {
        log(&setup).contains("starts with the system now").then_some(())
    });
    ui.click("button", "Close");
    wait_for("Settings to close", || {
        (app.cmd(json!({ "cmd": "window" }))["dialog"] == false).then_some(())
    });
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-," }));
    ui.wait_checked("Start with the system", true);
    ui.click("checkbox", "Start with the system");
    wait_for("the log", || {
        log(&setup).contains("no longer starts with the system").then_some(())
    });

    // Snapping separators to a grid can be turned off.
    ui.wait_checked("Snap separators to a grid", true);
    ui.click("checkbox", "Snap separators to a grid");
    wait_preference(&mut app, "snapToGrid", json!(false));

    // German, right away (also in the dialog itself) and after a restart.
    choose(&mut app, &ui, "Language", "Deutsch");
    ui.wait("combobox", "Sprache");
    ui.wait("button", "Neue Session");
    wait_preference(&mut app, "language", json!("de"));
    app.quit();

    let mut app = setup.launch();
    let ui = app.ui();
    ui.wait("button", "Neue Session");
    // Back to the system's language (English in the tests), from the gear this time.
    let gear = wait_for("the gear", || {
        ui.with_role("button")
            .into_iter()
            .find(|seen| seen.name.starts_with("Einstellungen"))
    });
    ui.click("button", &gear.name);
    choose(&mut app, &ui, "Sprache", "System (English)");
    ui.wait("button", "New session");
    ui.click("button", "Close");
    wait_for("Settings to close", || {
        (app.cmd(json!({ "cmd": "window" }))["dialog"] == false).then_some(())
    });
    app.quit();
}

#[test]
fn which_claude_code_runs_and_what_new_sessions_start_with() {
    let setup = Setup::new();
    // Another claude to choose (a copy of the fake one).
    let fake = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("fake-claude")
        .join("fake-claude.mjs")
        .canonicalize()
        .unwrap();
    let fake = fake.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    let other = setup.path("other-claude.mjs");
    std::fs::copy(&fake, &other).unwrap();
    let other = other.to_string_lossy().into_owned();

    let mut app = setup.launch();
    let ui = app.ui();
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-," }));
    // Without a choice, the one reframe finds (in the tests the one REFRAME_CLAUDE_BIN names).
    ui.wait("text", &format!("From the PATH: {fake}"));

    app.cmd(json!({ "cmd": "choose_file", "path": other }));
    ui.click("button", "Choose…");
    ui.wait("text", &format!("Chosen: {other}"));
    ui.focus("edit", "Model for new sessions");
    app.cmd(json!({ "cmd": "type", "text": "opus" }));
    ui.wait_value("edit", "Model for new sessions", "opus");
    choose(&mut app, &ui, "Permission mode for new sessions", "plan");
    wait_preference(&mut app, "claudeBin", json!(other));
    wait_preference(&mut app, "defaultModel", json!("opus"));
    wait_preference(&mut app, "defaultPermissionMode", json!("plan"));
    ui.click("button", "Close");
    wait_for("Settings to close", || {
        (app.cmd(json!({ "cmd": "window" }))["dialog"] == false).then_some(())
    });

    // "New session" has them filled in, and the session runs the chosen claude with them.
    app.cmd(json!({ "cmd": "open", "dialog": "new" }));
    ui.click("button", "Advanced");
    ui.wait_value("edit", "Model", "opus");
    ui.wait_value("combobox", "Permission mode", "plan");
    ui.focus("edit", "Folder");
    app.cmd(json!({ "cmd": "type", "text": setup.project() }));
    ui.focus("edit", "Name (optional)");
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-a" }));
    app.cmd(json!({ "cmd": "type", "text": "alpha" }));
    ui.click("button", "Start");
    let start = wait_for("the start", || {
        setup.fake_log().into_iter().find(|entry| entry["event"] == "start")
    });
    assert_eq!(start["script"], json!(other), "{start}");
    let started = args(&start);
    assert!(
        started.windows(2).any(|pair| pair == ["--model", "opus"]),
        "{started:?}"
    );
    assert!(
        started.windows(2).any(|pair| pair == ["--permission-mode", "plan"]),
        "{started:?}"
    );

    // Chosen and gone since: Settings says so; back to the PATH's.
    std::fs::remove_file(&other).unwrap();
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-," }));
    ui.wait("text", &format!("Chosen, but not there any more: {other}"));
    ui.click("button", "Use the one on the PATH");
    ui.wait("text", &format!("From the PATH: {fake}"));
    wait_preference(&mut app, "claudeBin", serde_json::Value::Null);
    app.quit();
}
