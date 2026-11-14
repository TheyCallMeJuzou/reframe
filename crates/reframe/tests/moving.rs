//! Moving things with the fake claude: groups, tiles dragged within a window and between
//! windows, tiles torn out into windows of their own, files dropped onto tiles, and what closing
//! a window does. What the Electron version's `e2e/groups.spec.ts`, `windows.spec.ts`,
//! `closed-windows.spec.ts` and `files.spec.ts` checked.
//!
//! A drag with the real pointer cannot be scripted, so the tests call what a drop calls, with
//! the point the pointer would be at.

mod common;

use common::{App, Setup, wait_for};
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

fn new_session(app: &mut App, setup: &Setup, name: &str) -> String {
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": name }));
    created["session"].as_str().unwrap().to_owned()
}

fn window_ids(app: &mut App) -> Vec<String> {
    app.state()["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|window| window["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Where a window drew its tiles and groups, once it shows that many tiles.
fn placed(app: &mut App, window: &str, tiles: usize) -> Value {
    wait_for(&format!("{tiles} tiles in the window"), || {
        let placed = app.cmd(json!({ "cmd": "placement", "window": window }));
        (placed["tiles"].as_array().is_some_and(|shown| shown.len() == tiles)).then_some(placed)
    })
}

fn tile_rect(placed: &Value, session: &str) -> [f64; 4] {
    let tiles = placed["tiles"].as_array().unwrap();
    let tile = tiles.iter().find(|tile| tile["session"] == session).unwrap();
    rect(&tile["rect"])
}

/// The sessions a window shows inside a group's frame.
fn grouped(placed: &Value) -> Vec<String> {
    let tiles = placed["tiles"].as_array().unwrap();
    tiles
        .iter()
        .filter(|tile| !tile["group"].is_null())
        .map(|tile| tile["session"].as_str().unwrap().to_owned())
        .collect()
}

fn starts_and_exits(setup: &Setup) -> (usize, usize) {
    let log = setup.fake_log();
    let count = |event: &str| log.iter().filter(|entry| entry["event"] == event).count();
    (count("start"), count("exit"))
}

fn labels(menu: &Value) -> Vec<&str> {
    menu.as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["label"].as_str().unwrap())
        .collect()
}

#[test]
fn groups_are_made_grown_renamed_recolored_collapsed_torn_out_dissolved_and_ended() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let ids: Vec<String> = ["alpha", "beta", "gamma", "delta"]
        .iter()
        .map(|name| new_session(&mut app, &setup, name))
        .collect();
    let (alpha, beta, gamma, delta) = (&ids[0], &ids[1], &ids[2], &ids[3]);
    for id in &ids {
        app.wait_status(id, "idle");
    }
    let main = window_ids(&mut app)[0].clone();
    placed(&mut app, &main, 4);

    // A tile's menu: "New group" frames alpha with an automatic name and colour.
    let menu = app.cmd(json!({ "cmd": "menu", "session": alpha }));
    assert_eq!(
        labels(&menu["menu"]),
        ["Rename", "New group", "Add to group", "Remove from group"]
    );
    assert_eq!(menu["menu"][2]["enabled"], false, "no group to add to yet");
    assert_eq!(menu["menu"][3]["enabled"], false, "not in a group yet");
    assert_eq!(
        app.cmd(json!({ "cmd": "menu_click", "label": "New group" }))["ok"],
        true
    );
    let shown = wait_for("the group's frame", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        (placed["groups"].as_array().is_some_and(|groups| groups.len() == 1)).then_some(placed)
    });
    let group = shown["groups"][0]["id"].as_str().unwrap().to_owned();
    let find_group = |app: &mut App| {
        fn find(node: &Value) -> Option<Value> {
            if node["kind"] == "group" {
                return Some(node.clone());
            }
            node["children"].as_array()?.iter().find_map(find)
        }
        find(&app.state()["windows"][0]["layout"])
    };
    let made = find_group(&mut app).unwrap();
    assert_eq!(made["name"], "Group 1");
    assert_eq!(made["color"], "#d97757");
    assert_eq!(grouped(&shown), std::slice::from_ref(alpha));

    // Dropping beta onto the right part of alpha (inside the frame) makes it join the group.
    let onto_alpha = |app: &mut App| {
        let shown = app.cmd(json!({ "cmd": "placement" }));
        let at = tile_rect(&shown, alpha);
        app.cmd(json!({
            "cmd": "drop", "session": beta, "x": at[0] + at[2] * 0.7, "y": at[1] + 30.0 + (at[3] - 30.0) / 2.0,
        }))
    };
    assert_eq!(onto_alpha(&mut app)["result"], "moved");
    wait_for("beta to be in the group", || {
        let shown = app.cmd(json!({ "cmd": "placement" }));
        (grouped(&shown).len() == 2).then_some(())
    });

    // Dropped on the frame's edge it goes next to the group again; then back in.
    let shown = app.cmd(json!({ "cmd": "placement" }));
    let frame = rect(&shown["groups"][0]["rect"]);
    let edge = json!({ "x": frame[0] + frame[2] - 4.0, "y": frame[1] + frame[3] / 2.0 });
    let spot = app.cmd(json!({ "cmd": "spot", "session": beta, "x": edge["x"], "y": edge["y"] }));
    assert_eq!(spot["target"], json!({ "key": group, "side": "right" }));
    let out = app.cmd(json!({ "cmd": "drop", "session": beta, "x": edge["x"], "y": edge["y"] }));
    assert_eq!(out["result"], "moved");
    wait_for("beta to be out of the group", || {
        let shown = app.cmd(json!({ "cmd": "placement" }));
        (grouped(&shown) == std::slice::from_ref(alpha) && shown["tiles"].as_array().unwrap().len() == 4).then_some(())
    });
    assert_eq!(onto_alpha(&mut app)["result"], "moved");
    wait_for("beta to be in the group again", || {
        (grouped(&app.cmd(json!({ "cmd": "placement" }))).len() == 2).then_some(())
    });

    // A group never lands inside another group, and nothing lands on itself.
    let shown = app.cmd(json!({ "cmd": "placement" }));
    let at = tile_rect(&shown, alpha);
    let middle = json!({ "x": at[0] + at[2] / 2.0, "y": at[1] + at[3] / 2.0 });
    let own = app.cmd(json!({ "cmd": "spot", "group": group, "x": middle["x"], "y": middle["y"] }));
    assert_eq!(own["target"], Value::Null);
    assert_eq!(own["preview"], Value::Null);

    // "Add to group" (the menu names the group) and "Remove from group".
    let menu = app.cmd(json!({ "cmd": "menu", "session": gamma }));
    assert_eq!(labels(&menu["menu"][2]["children"]), ["Group 1"]);
    assert_eq!(app.cmd(json!({ "cmd": "menu_click", "label": "Group 1" }))["ok"], true);
    wait_for("gamma to be in the group", || {
        (grouped(&app.cmd(json!({ "cmd": "placement" }))).len() == 3).then_some(())
    });
    app.cmd(json!({ "cmd": "menu", "session": gamma }));
    assert_eq!(
        app.cmd(json!({ "cmd": "menu_click", "label": "Remove from group" }))["ok"],
        true
    );
    wait_for("gamma to be out of the group", || {
        (grouped(&app.cmd(json!({ "cmd": "placement" }))).len() == 2).then_some(())
    });

    // Renamed in place, recoloured from the palette.
    app.cmd(json!({ "cmd": "rename_group", "group": group }));
    wait_for("the field for the group's name", || {
        (app.cmd(json!({ "cmd": "window" }))["renamingGroup"] == group.as_str()).then_some(())
    });
    app.cmd(json!({ "cmd": "type", "text": "API" }));
    app.cmd(json!({ "cmd": "key", "keys": "enter" }));
    wait_for("the group's new name", || {
        (find_group(&mut app).unwrap()["name"] == "API").then_some(())
    });
    let colors = app.cmd(json!({ "cmd": "menu", "group": group, "colors": true }));
    assert_eq!(colors["menu"].as_array().unwrap().len(), 8);
    assert_eq!(colors["menu"][0]["checked"], true, "its colour is the checked one");
    assert_eq!(app.cmd(json!({ "cmd": "menu_click", "label": "Blue" }))["ok"], true);
    assert_eq!(find_group(&mut app).unwrap()["color"], "#5b9cf0");
    let menu = app.cmd(json!({ "cmd": "menu", "group": group }));
    assert_eq!(
        labels(&menu["menu"]),
        ["Rename", "Color", "Dissolve group", "End all sessions"]
    );
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));
    wait_for("the menu to close", || {
        app.cmd(json!({ "cmd": "window" }))["menu"].is_null().then_some(())
    });

    // Collapsed, the group is one entry of the strip.
    app.cmd(json!({ "cmd": "layout", "action": "groupCollapse", "group": group }));
    let dock = app.state()["windows"][0]["dock"].clone();
    assert_eq!(dock[0]["kind"], "group");
    assert_eq!(dock[0]["node"]["name"], "API");
    placed(&mut app, &main, 2);
    app.quit();

    // --- restart: still collapsed, with its name and colour ---
    let mut app = setup.launch();
    let dock = app.state()["windows"][0]["dock"].clone();
    assert_eq!(dock[0]["node"]["name"], "API");
    assert_eq!(dock[0]["node"]["color"], "#5b9cf0");
    app.cmd(json!({ "cmd": "layout", "action": "groupExpand", "group": group }));
    let shown = placed(&mut app, &main, 4);
    assert_eq!(grouped(&shown).len(), 2);
    for id in &ids {
        app.wait_status(id, "idle");
    }

    // The whole group gets a window of its own, and comes back below delta.
    let screen = rect(&app.cmd(json!({ "cmd": "window" }))["screen"]);
    let outside = json!({ "x": screen[0] + screen[2] + 300.0, "y": screen[1] + 80.0 });
    let torn = app.cmd(json!({ "cmd": "tear_out", "group": group, "x": outside["x"], "y": outside["y"] }));
    assert_eq!(torn["ok"], true);
    let second = torn["window"].as_str().unwrap().to_owned();
    assert_eq!(window_ids(&mut app), [main.clone(), second.clone()]);
    assert_eq!(grouped(&placed(&mut app, &second, 2)).len(), 2);
    placed(&mut app, &main, 2);
    let target = json!({ "key": format!("tile:{delta}"), "side": "bottom" });
    let back = app.cmd(json!({ "cmd": "transfer", "group": group, "from": second, "to": main, "target": target }));
    assert_eq!(back["ok"], true);
    assert_eq!(window_ids(&mut app), std::slice::from_ref(&main));
    wait_for("the emptied window to close", || {
        (app.cmd(json!({ "cmd": "window", "window": second }))["error"] == "no window").then_some(())
    });
    assert_eq!(grouped(&placed(&mut app, &main, 4)).len(), 2);

    // Dissolved, the tiles stay; a fresh group of gamma and delta is ended as a whole.
    app.cmd(json!({ "cmd": "group", "action": "dissolve", "group": group }));
    assert!(find_group(&mut app).is_none());
    placed(&mut app, &main, 4);
    app.cmd(json!({ "cmd": "group", "action": "create", "session": gamma }));
    let next = find_group(&mut app).unwrap();
    assert_eq!(next["name"], "Group 1");
    let next = next["id"].as_str().unwrap().to_owned();
    app.cmd(json!({ "cmd": "group", "action": "add", "session": delta, "group": next }));
    app.cmd(json!({ "cmd": "group", "action": "endAll", "group": next }));
    placed(&mut app, &main, 2);
    assert_eq!(app.record(gamma)["lifecycle"], "ended");
    assert_eq!(app.record(delta)["lifecycle"], "ended");
    assert_eq!(app.record(beta)["lifecycle"], "open");

    app.quit();
    let (starts, exits) = starts_and_exits(&setup);
    assert_eq!(starts, exits);
}

#[test]
fn tiles_move_between_windows_empty_windows_close_and_all_come_back() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    let beta = new_session(&mut app, &setup, "beta");
    let gamma = new_session(&mut app, &setup, "gamma");
    for id in [&alpha, &beta, &gamma] {
        app.wait_status(id, "idle");
    }
    let main = window_ids(&mut app)[0].clone();
    let shown = placed(&mut app, &main, 3);

    // Within the window: beta dropped onto the lower half of alpha goes below it.
    let at = tile_rect(&shown, &alpha);
    let low = json!({ "x": at[0] + at[2] / 2.0, "y": at[1] + at[3] - 20.0 });
    let spot = app.cmd(json!({ "cmd": "spot", "session": beta, "x": low["x"], "y": low["y"] }));
    assert_eq!(
        spot["target"],
        json!({ "key": format!("tile:{alpha}"), "side": "bottom" })
    );
    let preview = rect(&spot["preview"]);
    assert!(
        (preview[3] - (at[3] - 30.0) / 2.0).abs() < 0.01,
        "half of the tile's body"
    );
    assert_eq!(
        app.cmd(json!({ "cmd": "drop", "session": beta, "x": low["x"], "y": low["y"] }))["result"],
        "moved"
    );
    wait_for("beta below alpha", || {
        let shown = app.cmd(json!({ "cmd": "placement" }));
        let (a, b) = (tile_rect(&shown, &alpha), tile_rect(&shown, &beta));
        (a[0] == b[0] && b[1] > a[1]).then_some(())
    });
    // The toolbar takes no drop.
    assert_eq!(
        app.cmd(json!({ "cmd": "drop", "session": beta, "x": 300.0, "y": 10.0 }))["result"],
        "nothing"
    );

    // Letting go over one of reframe's windows is not a tear-out.
    let screen = rect(&app.cmd(json!({ "cmd": "window" }))["screen"]);
    let inside = json!({ "x": screen[0] + 300.0, "y": screen[1] + 200.0 });
    let outside = json!({ "x": screen[0] + screen[2] + 300.0, "y": screen[1] + 80.0 });
    let kept = app.cmd(json!({ "cmd": "tear_out", "session": alpha, "x": inside["x"], "y": inside["y"] }));
    assert_eq!(kept["ok"], false);

    // Torn out, beta has a window of its own, in the same process: no claude is started anew.
    let torn = app.cmd(json!({ "cmd": "tear_out", "session": beta, "x": outside["x"], "y": outside["y"] }));
    assert_eq!(torn["ok"], true);
    let second = torn["window"].as_str().unwrap().to_owned();
    placed(&mut app, &second, 1);
    placed(&mut app, &main, 2);
    assert_eq!(app.status(&beta), "idle");
    assert_eq!(starts_and_exits(&setup), (3, 0));

    // Moved back below alpha: the second window is empty now and closes.
    let target = json!({ "key": format!("tile:{alpha}"), "side": "bottom" });
    app.cmd(json!({ "cmd": "transfer", "session": beta, "from": second, "to": main, "target": target }));
    assert_eq!(window_ids(&mut app), std::slice::from_ref(&main));
    placed(&mut app, &main, 3);
    wait_for("the emptied window to close", || {
        (app.cmd(json!({ "cmd": "windows" })).as_array().unwrap().len() == 1).then_some(())
    });

    // Gamma torn out and collapsed there: a window with only a collapsed tile stays.
    let torn = app.cmd(json!({ "cmd": "tear_out", "session": gamma, "x": outside["x"], "y": outside["y"] }));
    let third = torn["window"].as_str().unwrap().to_owned();
    placed(&mut app, &third, 1);
    app.cmd(json!({ "cmd": "layout", "action": "collapse", "session": gamma, "window": third }));
    assert_eq!(app.state()["windows"][1]["dock"][0]["sessionId"], gamma.as_str());
    assert_eq!(window_ids(&mut app), [main.clone(), third.clone()]);
    app.quit();

    // --- restart: both windows come back, gamma still collapsed in the second ---
    let mut app = setup.launch();
    assert_eq!(window_ids(&mut app), [main.clone(), third.clone()]);
    wait_for("both windows to be open", || {
        let windows = app.cmd(json!({ "cmd": "windows" }));
        (windows.as_array().unwrap().iter().all(|window| window["open"] == true)).then_some(())
    });
    placed(&mut app, &main, 2);
    placed(&mut app, &third, 0);

    // A window without tiles takes a dropped tile as a whole, wherever in its area.
    let area = rect(&app.cmd(json!({ "cmd": "placement", "window": third }))["area"]);
    let middle = json!({ "x": area[0] + area[2] / 2.0, "y": area[1] + area[3] / 2.0 });
    let spot = app.cmd(json!({ "cmd": "spot", "window": third, "session": alpha, "x": middle["x"], "y": middle["y"] }));
    assert_eq!(spot["target"], Value::Null);
    assert_eq!(rect(&spot["preview"]), area);
    let moved = app.cmd(json!({
        "cmd": "drop", "window": main, "to": third, "session": alpha, "x": middle["x"], "y": middle["y"],
    }));
    assert_eq!(moved["result"], "transferred");
    placed(&mut app, &third, 1);

    // The first window always stays, also with nothing left in it.
    let shown = placed(&mut app, &third, 1);
    let at = tile_rect(&shown, &alpha);
    let right = json!({ "x": at[0] + at[2] - 30.0, "y": at[1] + at[3] / 2.0 });
    let moved = app.cmd(json!({
        "cmd": "drop", "window": main, "to": third, "session": beta, "x": right["x"], "y": right["y"],
    }));
    assert_eq!(moved["result"], "transferred");
    placed(&mut app, &third, 2);
    placed(&mut app, &main, 0);
    assert_eq!(window_ids(&mut app), [main.clone(), third.clone()]);
    assert_eq!(app.state()["windows"][0]["layout"], Value::Null);

    // And takes a tile back.
    let area = rect(&app.cmd(json!({ "cmd": "placement", "window": main }))["area"]);
    let moved = app.cmd(json!({
        "cmd": "drop", "window": third, "to": main, "session": alpha,
        "x": area[0] + area[2] / 2.0, "y": area[1] + area[3] / 2.0,
    }));
    assert_eq!(moved["result"], "transferred");
    placed(&mut app, &main, 1);
    for id in [&alpha, &beta, &gamma] {
        app.wait_status(id, "idle");
    }

    app.quit();
    let (starts, exits) = starts_and_exits(&setup);
    assert_eq!(starts, 6, "three sessions, started once and resumed once");
    assert_eq!(starts, exits);
}

#[test]
fn closing_a_window_with_sessions_asks_and_empty_windows_just_go() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    let beta = new_session(&mut app, &setup, "beta");
    for id in [&alpha, &beta] {
        app.wait_status(id, "idle");
    }
    let main = window_ids(&mut app)[0].clone();
    let shown = |app: &mut App, window: &str| app.cmd(json!({ "cmd": "window", "window": window }));
    let open = |app: &mut App, window: &str| {
        wait_for("the new window to be drawn", || {
            (app.cmd(json!({ "cmd": "window", "window": window }))["drawn"] == true).then_some(())
        });
    };

    // A new window (Ctrl+Shift+N) closed while it is empty is gone, nothing asked.
    wait_for("the window to take keys", || {
        (shown(&mut app, &main)["keyboard"] == true).then_some(())
    });
    assert_eq!(
        app.cmd(json!({ "cmd": "key", "keys": "ctrl-shift-n" }))["handled"],
        true
    );
    let empty = wait_for("the new window", || window_ids(&mut app).get(1).cloned());
    open(&mut app, &empty);
    app.cmd(json!({ "cmd": "close", "window": empty }));
    assert_eq!(window_ids(&mut app), std::slice::from_ref(&main));
    wait_for("the empty window to close", || {
        (app.cmd(json!({ "cmd": "windows" })).as_array().unwrap().len() == 1).then_some(())
    });

    // One with a session in it asks. Cancel: it stays open.
    let second = app.cmd(json!({ "cmd": "new_window" }))["window"]
        .as_str()
        .unwrap()
        .to_owned();
    open(&mut app, &second);
    app.cmd(json!({ "cmd": "transfer", "session": beta, "from": main, "to": second }));
    placed(&mut app, &second, 1);
    app.cmd(json!({ "cmd": "close", "window": second }));
    wait_for("the question", || {
        let window = shown(&mut app, &second);
        (window["asking"] == true && window["dialog"] == true).then_some(())
    });
    // A second click on the X does not ask twice.
    app.cmd(json!({ "cmd": "close", "window": second }));
    app.cmd(json!({ "cmd": "close_answer", "window": second }));
    wait_for("the question to go", || {
        let window = shown(&mut app, &second);
        (window["asking"] == false && window["dialog"] == false).then_some(())
    });
    assert_eq!(shown(&mut app, &second)["away"], false);
    assert_eq!(app.state()["preferences"]["closeWindow"], "ask");

    // Keep: the window is put away, beta runs on.
    app.cmd(json!({ "cmd": "close", "window": second }));
    app.cmd(json!({ "cmd": "close_answer", "window": second, "choice": "keep" }));
    wait_for("the window to be put away", || {
        (shown(&mut app, &second)["away"] == true).then_some(())
    });
    assert_eq!(app.record(&beta)["lifecycle"], "open");
    assert_eq!(app.status(&beta), "idle");
    assert_eq!(starts_and_exits(&setup).1, 0);

    // End, and do not ask again: beta ends (it stays in the list) and the window is gone.
    app.cmd(json!({ "cmd": "close", "window": second }));
    app.cmd(json!({ "cmd": "close_answer", "window": second, "choice": "end", "remember": true }));
    assert_eq!(window_ids(&mut app), std::slice::from_ref(&main));
    assert_eq!(app.record(&beta)["lifecycle"], "ended");
    assert_eq!(app.state()["preferences"]["closeWindow"], "end");
    wait_for("beta's claude to exit", || {
        (starts_and_exits(&setup).1 == 1).then_some(())
    });

    // From now on without a question: closing the first window ends alpha. The window stays
    // in the state, empty; with no window left on screen, reframe quits.
    app.cmd(json!({ "cmd": "close", "window": main }));
    let child = &mut app.child;
    let status = wait_for("reframe to quit", || child.try_wait().unwrap());
    assert!(status.success());
    let saved = setup.saved_state();
    assert_eq!(saved["windows"].as_array().unwrap().len(), 1);
    assert_eq!(saved["windows"][0]["layout"], Value::Null);
    assert_eq!(saved["sessions"][&alpha]["lifecycle"], "ended");
    assert_eq!(starts_and_exits(&setup), (2, 2));
}

#[test]
fn empty_windows_are_not_brought_back_and_files_are_typed_in_as_paths() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    app.wait_status(&alpha, "idle");
    let main = window_ids(&mut app)[0].clone();
    placed(&mut app, &main, 1);

    // Files dropped onto a tile: their paths, quoted where a shell would need it.
    app.cmd(json!({ "cmd": "drop_files", "session": alpha, "paths": ["C:\\shots\\a.png", "C:\\My Pictures\\b.png"] }));
    app.wait_screen(&alpha, "C:\\shots\\a.png \"C:\\My Pictures\\b.png\"");
    wait_for("the tile to take the keyboard", || {
        (app.cmd(json!({ "cmd": "window" }))["focused"] == alpha.as_str()).then_some(())
    });

    // alpha moves to a second window, which leaves the first one empty; a third is left empty.
    let second = app.cmd(json!({ "cmd": "new_window" }))["window"]
        .as_str()
        .unwrap()
        .to_owned();
    app.cmd(json!({ "cmd": "transfer", "session": alpha, "from": main, "to": second }));
    placed(&mut app, &second, 1);
    let third = app.cmd(json!({ "cmd": "new_window" }))["window"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(window_ids(&mut app), [main, second.clone(), third]);
    app.quit();

    // Only the window with alpha comes back, as the first window now.
    let mut app = setup.launch();
    assert_eq!(window_ids(&mut app), std::slice::from_ref(&second));
    app.wait_status(&alpha, "idle");
    placed(&mut app, &second, 1);
    app.quit();
}

/// Waits until the window has drawn what the last event changed (and so listens to the next
/// one): until it stops drawing.
fn next_frame(app: &mut App) {
    let frames = |app: &mut App| app.cmd(json!({ "cmd": "window" }))["frames"].as_u64().unwrap();
    wait_for("the window to come to rest", || {
        let before = frames(app);
        std::thread::sleep(std::time::Duration::from_millis(120));
        (frames(app) == before).then_some(())
    });
}

#[test]
fn the_pointer_drags_tiles_opens_menus_and_brings_files() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    let beta = new_session(&mut app, &setup, "beta");
    for id in [&alpha, &beta] {
        app.wait_status(id, "idle");
    }
    let main = window_ids(&mut app)[0].clone();
    let shown = placed(&mut app, &main, 2);
    let (a, b) = (tile_rect(&shown, &alpha), tile_rect(&shown, &beta));
    assert!(b[0] > a[0], "beta is at the right of alpha");
    let mouse = |app: &mut App, kind: &str, x: f64, y: f64| {
        app.cmd(json!({ "cmd": "mouse", "type": kind, "x": x, "y": y, "held": true }));
    };
    // In beta's bar, on its path (not on a button or the name).
    let bar = (b[0] + b[2] * 0.6, b[1] + 15.0);

    // A press that does not move is no drag.
    mouse(&mut app, "down", bar.0, bar.1);
    next_frame(&mut app);
    mouse(&mut app, "move", bar.0 + 2.0, bar.1 + 1.0);
    assert_eq!(app.cmd(json!({ "cmd": "window" }))["dragging"], Value::Null);
    mouse(&mut app, "up", bar.0 + 2.0, bar.1 + 1.0);

    // Dragged over the lower part of alpha: the name follows the pointer and the lower half of
    // alpha shows where it would go.
    let low = (a[0] + a[2] / 2.0, a[1] + a[3] - 40.0);
    mouse(&mut app, "down", bar.0, bar.1);
    next_frame(&mut app);
    mouse(&mut app, "move", bar.0 - 40.0, bar.1 + 20.0);
    mouse(&mut app, "move", low.0, low.1);
    let dragged = app.cmd(json!({ "cmd": "window" }));
    assert_eq!(dragged["dragging"], "beta");
    let preview = rect(&dragged["preview"]);
    assert_eq!(preview[0], a[0]);
    assert!((preview[1] + preview[3] - (a[1] + a[3])).abs() < 0.01, "the lower half");

    // Escape gives the drag up: nothing moves.
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));
    let given_up = app.cmd(json!({ "cmd": "window" }));
    assert_eq!(given_up["dragging"], Value::Null);
    assert_eq!(given_up["preview"], Value::Null);
    mouse(&mut app, "up", low.0, low.1);
    assert_eq!(app.state()["windows"][0]["layout"]["dir"], "row");

    // Let go there, beta sits below alpha.
    mouse(&mut app, "down", bar.0, bar.1);
    next_frame(&mut app);
    mouse(&mut app, "move", bar.0 - 40.0, bar.1 + 20.0);
    mouse(&mut app, "move", low.0, low.1);
    next_frame(&mut app);
    mouse(&mut app, "up", low.0, low.1);
    wait_for("beta below alpha", || {
        let shown = app.cmd(json!({ "cmd": "placement" }));
        let (a, b) = (tile_rect(&shown, &alpha), tile_rect(&shown, &beta));
        (a[0] == b[0] && b[1] > a[1]).then_some(())
    });
    let after = app.cmd(json!({ "cmd": "window" }));
    assert_eq!(after["dragging"], Value::Null);
    assert_eq!(after["preview"], Value::Null);

    // The right button on a tile's bar opens its menu; a click beside it closes it.
    let shown = placed(&mut app, &main, 2);
    let a = tile_rect(&shown, &alpha);
    app.cmd(json!({ "cmd": "mouse", "type": "down", "button": "right", "x": a[0] + a[2] * 0.6, "y": a[1] + 15.0 }));
    let menu = wait_for("the tile's menu", || {
        let menu = app.cmd(json!({ "cmd": "window" }))["menu"].clone();
        (!menu.is_null()).then_some(menu)
    });
    assert_eq!(labels(&menu)[1], "New group");
    next_frame(&mut app);
    mouse(&mut app, "down", a[0] + a[2] - 60.0, a[1] + a[3] - 60.0);
    wait_for("the menu to close", || {
        app.cmd(json!({ "cmd": "window" }))["menu"].is_null().then_some(())
    });
    mouse(&mut app, "up", a[0] + a[2] - 60.0, a[1] + a[3] - 60.0);

    // Files dragged in from outside: the tile under them says that it takes them, and gets
    // their paths typed in when they are let go.
    let b = tile_rect(&shown, &beta);
    let (over_a, over_b) = ((a[0] + 200.0, a[1] + 120.0), (b[0] + 200.0, b[1] + 120.0));
    app.cmd(json!({
        "cmd": "file_drag", "type": "enter", "x": over_a.0, "y": over_a.1, "paths": ["C:\\shots\\one two.png"],
    }));
    wait_for("alpha to take the files", || {
        (app.cmd(json!({ "cmd": "window" }))["filesOver"] == alpha.as_str()).then_some(())
    });
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "file_drag", "type": "move", "x": over_b.0, "y": over_b.1 }));
    wait_for("beta to take the files", || {
        (app.cmd(json!({ "cmd": "window" }))["filesOver"] == beta.as_str()).then_some(())
    });
    next_frame(&mut app);
    app.cmd(json!({ "cmd": "file_drag", "type": "drop", "x": over_b.0, "y": over_b.1 }));
    app.wait_screen(&beta, "\"C:\\shots\\one two.png\"");
    assert!(!app.screen(&alpha).contains("one two"));
    wait_for("the mark to go", || {
        app.cmd(json!({ "cmd": "window" }))["filesOver"].is_null().then_some(())
    });

    // The bar still takes clicks: a double click on the name edits it.
    let name = (a[0] + 80.0, a[1] + 15.0);
    for clicks in [1, 2] {
        for kind in ["down", "up"] {
            app.cmd(json!({ "cmd": "mouse", "type": kind, "x": name.0, "y": name.1, "clicks": clicks }));
        }
    }
    wait_for("the field for alpha's name", || {
        (app.cmd(json!({ "cmd": "window" }))["renaming"] == alpha.as_str()).then_some(())
    });
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));

    // Out of the window, over another one: that one shows where the tile would go, and gets
    // it. The second window lies 40 pixels to the right of and below the first.
    app.cmd(json!({ "cmd": "resize", "width": 700.0, "height": 500.0 }));
    wait_for("the smaller window to be saved", || {
        (app.state()["windows"][0]["bounds"]["width"] == 700.0).then_some(())
    });
    let second = app.cmd(json!({ "cmd": "new_window" }))["window"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_for("the second window", || {
        (app.cmd(json!({ "cmd": "window", "window": second }))["drawn"] == true).then_some(())
    });
    let (first, other) = (
        rect(&app.cmd(json!({ "cmd": "window" }))["screen"]),
        rect(&app.cmd(json!({ "cmd": "window", "window": second }))["screen"]),
    );
    assert_eq!((other[0] - first[0], other[1] - first[1]), (40.0, 40.0));
    let shown = placed(&mut app, &main, 2);
    let a = tile_rect(&shown, &alpha);
    let bar = (a[0] + a[2] * 0.6, a[1] + 15.0);
    // In the strip of the second window that the first does not cover.
    let over = (first[2] + 20.0, 300.0);
    mouse(&mut app, "down", bar.0, bar.1);
    next_frame(&mut app);
    mouse(&mut app, "move", bar.0 + 30.0, bar.1 + 30.0);
    mouse(&mut app, "move", over.0, over.1);
    let there = app.cmd(json!({ "cmd": "window", "window": second }));
    let area = rect(&app.cmd(json!({ "cmd": "placement", "window": second }))["area"]);
    assert_eq!(rect(&there["preview"]), area, "the empty window takes it as a whole");
    assert_eq!(app.cmd(json!({ "cmd": "window" }))["preview"], Value::Null);
    next_frame(&mut app);
    mouse(&mut app, "up", over.0, over.1);
    placed(&mut app, &second, 1);
    placed(&mut app, &main, 1);
    assert_eq!(
        app.cmd(json!({ "cmd": "window", "window": second }))["preview"],
        Value::Null
    );

    // Let go outside of both, beta gets a window of its own there.
    let shown = placed(&mut app, &main, 1);
    let b = tile_rect(&shown, &beta);
    let bar = (b[0] + b[2] * 0.6, b[1] + 15.0);
    let away = (first[2] + 400.0, 120.0);
    mouse(&mut app, "down", bar.0, bar.1);
    next_frame(&mut app);
    mouse(&mut app, "move", bar.0 + 30.0, bar.1 + 30.0);
    mouse(&mut app, "move", away.0, away.1);
    next_frame(&mut app);
    mouse(&mut app, "up", away.0, away.1);
    let windows = wait_for("a third window", || {
        let ids = window_ids(&mut app);
        (ids.len() == 3).then_some(ids)
    });
    placed(&mut app, &windows[2], 1);
    placed(&mut app, &main, 0);
    // Its title bar is under the pointer.
    let bounds = app.state()["windows"][2]["bounds"].clone();
    assert_eq!(bounds["x"], first[0] + away.0 - 60.0);
    assert_eq!(bounds["y"], first[1] + away.1 - 16.0);
    assert_eq!(starts_and_exits(&setup), (2, 0));
    app.quit();
}
