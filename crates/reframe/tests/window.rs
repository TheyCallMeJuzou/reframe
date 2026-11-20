//! The window's layout with the fake claude: tiles side by side, collapsed and brought back,
//! zoomed, and separators dragged on and off the grid. What the Electron version's
//! `e2e/sessions.spec.ts` (layout parts) and `zoom.spec.ts` checked.

mod common;

use common::{Setup, next_frame, wait_for};
use serde_json::{Value, json};

fn rect(value: &Value) -> [f64; 4] {
    let numbers: Vec<f64> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|number| number.as_f64().unwrap())
        .collect();
    [numbers[0], numbers[1], numbers[2], numbers[3]]
}

#[test]
fn tiles_are_laid_out_collapsed_zoomed_and_resized() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project() }));
        ids.push(created["session"].as_str().unwrap().to_owned());
    }
    for id in &ids {
        app.wait_status(id, "idle");
    }

    // Two tiles side by side share the area, with a separator between them.
    let placed = wait_for("the window to draw two tiles", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        (placed["tiles"].as_array().is_some_and(|tiles| tiles.len() == 2)).then_some(placed)
    });
    let area = rect(&placed["area"]);
    let (left, right) = (rect(&placed["tiles"][0]["rect"]), rect(&placed["tiles"][1]["rect"]));
    assert_eq!(placed["tiles"][0]["session"], ids[0].as_str());
    assert_eq!(left[0], area[0]);
    assert_eq!(left[2], right[2], "equal shares");
    assert!((left[2] + 6.0 + right[2] - area[2]).abs() < 0.01);
    let split = placed["separators"][0]["split"].as_str().unwrap().to_owned();

    // Dragged with Alt, the separator goes exactly where the pointer goes.
    app.cmd(json!({ "cmd": "drag", "split": split, "index": 0, "delta": 37.0, "free": true }));
    let moved = wait_for("the separator to move", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        (rect(&placed["tiles"][0]["rect"])[2] != left[2]).then_some(placed)
    });
    assert!((rect(&moved["tiles"][0]["rect"])[2] - (left[2] + 37.0)).abs() < 0.01);
    let saved = app.state()["windows"][0]["layout"]["sizes"].clone();
    assert!((saved[0].as_f64().unwrap() + saved[1].as_f64().unwrap() - 100.0).abs() < 1e-6);

    // Dragged without, it lands on one of the twelve grid lines across the layout.
    app.cmd(json!({ "cmd": "drag", "split": split, "index": 0, "delta": 60.0 }));
    let snapped = wait_for("the separator to snap", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        (rect(&placed["separators"][0]["rect"])[0] != rect(&moved["separators"][0]["rect"])[0]).then_some(placed)
    });
    let separator = rect(&snapped["separators"][0]["rect"]);
    let steps = (separator[0] + separator[2] / 2.0 - area[0]) / (area[2] / 12.0);
    assert!((steps - steps.round()).abs() < 1e-6, "not on the grid: {steps}");

    // A panel keeps its smallest width however far the pointer goes.
    app.cmd(json!({ "cmd": "drag", "split": split, "index": 0, "delta": 5000.0, "free": true }));
    wait_for("the separator to stop at the smallest width", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        ((rect(&placed["tiles"][1]["rect"])[2] - 160.0).abs() < 0.01).then_some(())
    });

    // Zoomed, a tile fills the area alone; the layout itself is untouched.
    app.cmd(json!({ "cmd": "zoom", "session": ids[1] }));
    let zoomed = wait_for("the zoomed tile", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        (placed["zoomed"] == ids[1].as_str() && placed["tiles"].as_array().unwrap().len() == 1).then_some(placed)
    });
    assert_eq!(rect(&zoomed["tiles"][0]["rect"]), area);
    assert_eq!(app.state()["windows"][0]["layout"]["kind"], "split");
    app.cmd(json!({ "cmd": "zoom", "session": ids[1] }));
    wait_for("both tiles again", || {
        (app.cmd(json!({ "cmd": "placement" }))["tiles"]
            .as_array()
            .unwrap()
            .len()
            == 2)
            .then_some(())
    });

    // Collapsed, a tile leaves the layout for the strip and its session keeps running.
    app.cmd(json!({ "cmd": "layout", "action": "collapse", "session": ids[0] }));
    let state = app.state();
    assert_eq!(
        state["windows"][0]["layout"],
        json!({ "kind": "tile", "sessionId": ids[1] })
    );
    assert_eq!(state["windows"][0]["dock"][0]["sessionId"], ids[0].as_str());
    assert_eq!(app.status(&ids[0]), "idle");
    wait_for("the remaining tile to fill the area", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        let tiles = placed["tiles"].as_array().unwrap();
        (tiles.len() == 1 && rect(&tiles[0]["rect"])[2] == rect(&placed["area"])[2]).then_some(())
    });

    // Brought back, it sits where it was.
    app.cmd(json!({ "cmd": "layout", "action": "expand", "session": ids[0] }));
    let state = app.state();
    assert_eq!(state["windows"][0]["dock"], json!([]));
    assert_eq!(
        state["windows"][0]["layout"]["children"][0]["sessionId"],
        ids[0].as_str()
    );
    app.quit();
}

fn window(app: &mut common::App) -> Value {
    app.cmd(json!({ "cmd": "window" }))
}

fn key(app: &mut common::App, keys: &str) {
    let answer = app.cmd(json!({ "cmd": "key", "keys": keys }));
    assert_eq!(answer["handled"], true, "{keys} was not taken");
}

/// What `e2e/shortcuts.spec.ts`, `new-session.spec.ts` and `text-size.spec.ts` checked.
#[test]
fn shortcuts_reach_dialogs_tiles_and_the_text_size() {
    let setup = Setup::new();
    let mut app = setup.launch();
    wait_for("the window to be drawn", || {
        (window(&mut app)["drawn"] == true).then_some(())
    });

    // Ctrl+Shift+T opens the dialog in an empty window; a folder typed and Enter start a session
    // named after the folder, and its tile has the keyboard.
    key(&mut app, "ctrl-shift-t");
    wait_for("the New session dialog", || {
        (window(&mut app)["dialog"] == true).then_some(())
    });
    // Drawn, its folder field has the keyboard (a user types into what they see). The dialog is
    // drawn by the component library's root, not the window's view, so its frame is waited for
    // where a screen reader sees it.
    app.ui().wait("edit", "Folder");
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "type", "text": setup.project() }));
    key(&mut app, "enter");
    // Without a session, what the dialog shows says why (its fields, an error).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let first = loop {
        let state = app.state();
        if let Some(session) = state["sessions"].as_object().unwrap().values().next() {
            assert_eq!(session["name"], "project");
            break session["id"].as_str().unwrap().to_owned();
        }
        if std::time::Instant::now() > deadline {
            let shown: Vec<_> = app
                .ui()
                .all()
                .into_iter()
                .filter(|seen| matches!(seen.role, "edit" | "text" | "button"))
                .map(|seen| format!("{} {:?} {:?}", seen.role, seen.name, seen.value))
                .collect();
            panic!(
                "no session from the dialog; it shows {shown:#?}; the window: {}",
                window(&mut app)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    wait_for("the dialog to close and the tile to take the keyboard", || {
        let shown = window(&mut app);
        (shown["dialog"] == false && shown["focused"] == first.as_str()).then_some(())
    });
    assert_eq!(app.state()["recentFolders"][0], setup.project().as_str());

    let second = app.cmd(json!({ "cmd": "new", "cwd": setup.project() }))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    app.wait_status(&second, "idle");
    let focused = |app: &mut common::App, id: &str, what: &str| {
        wait_for(what, || (window(app)["focused"] == id).then_some(()));
    };
    focused(&mut app, &second, "the new tile to take the keyboard");

    // Ctrl+1…9 by place, Ctrl+Shift+arrows by direction.
    key(&mut app, "ctrl-1");
    focused(&mut app, &first, "Ctrl+1 to reach the first tile");
    key(&mut app, "ctrl-shift-right");
    focused(&mut app, &second, "Ctrl+Shift+Right to reach the tile at the right");
    key(&mut app, "ctrl-shift-left");
    focused(&mut app, &first, "Ctrl+Shift+Left to go back");

    // The text size: a step per press, saved as a preference, Ctrl+0 for normal.
    let size = |app: &mut common::App| app.state()["preferences"]["fontSize"].as_u64().unwrap();
    key(&mut app, "ctrl-=");
    key(&mut app, "ctrl-=");
    assert_eq!(size(&mut app), 15);
    key(&mut app, "ctrl--");
    assert_eq!(size(&mut app), 14);
    key(&mut app, "ctrl-0");
    assert_eq!(size(&mut app), 13);

    // Ctrl+Shift+Z fills the window with the focused tile and shows all again.
    key(&mut app, "ctrl-shift-z");
    wait_for("the zoom", || {
        (app.cmd(json!({ "cmd": "placement" }))["zoomed"] == first.as_str()).then_some(())
    });
    key(&mut app, "ctrl-shift-z");
    wait_for("the zoom to end", || {
        app.cmd(json!({ "cmd": "placement" }))["zoomed"].is_null().then_some(())
    });

    // Ctrl+Shift+M collapses the focused tile; pressed again with no tile focused, it brings
    // the one collapsed last back, with the keyboard.
    key(&mut app, "ctrl-shift-m");
    assert_eq!(app.state()["windows"][0]["dock"][0]["sessionId"], first.as_str());
    wait_for("no tile to have the keyboard", || {
        let shown = window(&mut app);
        (shown["focused"].is_null() && shown["keyboard"] == true).then_some(())
    });
    key(&mut app, "ctrl-shift-m");
    assert_eq!(app.state()["windows"][0]["dock"], json!([]));
    focused(&mut app, &first, "the tile brought back to take the keyboard");

    // Ctrl+, opens Settings; while a dialog is open the shortcuts are its keys.
    key(&mut app, "ctrl-,");
    wait_for("Settings", || (window(&mut app)["dialog"] == true).then_some(()));
    app.cmd(json!({ "cmd": "key", "keys": "ctrl-shift-m" }));
    assert_eq!(app.state()["windows"][0]["dock"], json!([]));
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));
    wait_for("Settings to close", || {
        (window(&mut app)["dialog"] == false).then_some(())
    });
    focused(&mut app, &first, "the tile to have the keyboard again");
    app.quit();
}

/// What `e2e/find.spec.ts` and `rename.spec.ts` checked.
#[test]
fn text_is_found_and_a_session_renamed_in_its_tile() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let id = app.cmd(json!({ "cmd": "new", "cwd": setup.project() }))["session"]
        .as_str()
        .unwrap()
        .to_owned();
    app.wait_status(&id, "idle");
    wait_for("the tile to take the keyboard", || {
        (window(&mut app)["focused"] == id.as_str()).then_some(())
    });

    // Ctrl+Shift+F opens the find bar in the focused tile; what is typed is found without regard
    // to case and counted.
    key(&mut app, "ctrl-shift-f");
    wait_for("the find bar", || {
        (window(&mut app)["find"]["session"] == id.as_str()).then_some(())
    });
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "type", "text": "fake cl" }));
    let found = wait_for("the text to be found", || {
        let find = window(&mut app)["find"].clone();
        (find["text"] == "fake cl" && find["count"] == 1).then_some(find)
    });
    assert_eq!(found["index"], 0);
    app.cmd(json!({ "cmd": "type", "text": "zzz" }));
    wait_for("nothing to be found", || {
        (window(&mut app)["find"]["count"] == 0).then_some(())
    });
    // Typed in the bar, the text did not reach the session.
    assert!(!app.screen(&id).contains("zzz"));
    // Esc closes the bar and the session is typed in again.
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));
    wait_for("the find bar to close", || {
        let shown = window(&mut app);
        (shown["find"].is_null() && shown["focused"] == id.as_str()).then_some(())
    });

    // A name typed in the tile's header is reframe's own; Claude Code keeps the one it had.
    app.cmd(json!({ "cmd": "rename", "session": id }));
    wait_for("the name field", || {
        (window(&mut app)["renaming"] == id.as_str()).then_some(())
    });
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "type", "text": "Mine" }));
    key(&mut app, "enter");
    wait_for("the name to be kept", || {
        let shown = window(&mut app);
        (shown["renaming"].is_null() && shown["focused"] == id.as_str()).then_some(())
    });
    let session = app.state()["sessions"][&id].clone();
    assert_eq!(session["name"], "Mine");
    assert!(session["claudeName"].is_null() && session.as_object().unwrap().contains_key("claudeName"));

    // Esc drops a change.
    app.cmd(json!({ "cmd": "rename", "session": id }));
    wait_for("the name field", || {
        (window(&mut app)["renaming"] == id.as_str()).then_some(())
    });
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "type", "text": "Other" }));
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));
    wait_for("the field to go", || {
        window(&mut app)["renaming"].is_null().then_some(())
    });
    assert_eq!(app.state()["sessions"][&id]["name"], "Mine");

    // An empty name goes back to the automatic one.
    app.cmd(json!({ "cmd": "rename", "session": id }));
    wait_for("the name field", || {
        (window(&mut app)["renaming"] == id.as_str()).then_some(())
    });
    key(&mut app, "backspace");
    key(&mut app, "enter");
    wait_for("the field to go", || {
        window(&mut app)["renaming"].is_null().then_some(())
    });
    assert!(app.state()["sessions"][&id].get("name").is_none());
    app.quit();
}

/// A window comes back as big as it was. The state keeps the whole window, frame and all, as
/// 0.x did (`e2e/windows.spec.ts` sets a window's bounds that way).
#[test]
fn a_window_opens_as_big_as_it_was_left() {
    let setup = Setup::new();
    let mut app = setup.launch();
    app.cmd(json!({ "cmd": "resize", "width": 910.0, "height": 640.0 }));
    let frame = window(&mut app)["frame"].clone();
    let (frame_width, frame_height) = (frame[0].as_f64().unwrap(), frame[1].as_f64().unwrap());
    assert!(
        frame_width > 0.0 && frame_height > 20.0,
        "a frame with a title bar: {frame}"
    );
    wait_for("the size to be remembered", || {
        let bounds = app.state()["windows"][0]["bounds"].clone();
        (bounds["width"] == (910.0 + frame_width).round() && bounds["height"] == (640.0 + frame_height).round())
            .then_some(())
    });
    app.quit();

    let mut app = setup.launch();
    wait_for("the window to be that big again", || {
        (window(&mut app)["size"] == json!([910.0, 640.0])).then_some(())
    });
    app.quit();
}
