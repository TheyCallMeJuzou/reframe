//! The window's layout with the fake claude: tiles side by side, collapsed and brought back,
//! zoomed, and separators dragged on and off the grid. What the Electron version's
//! `e2e/sessions.spec.ts` (layout parts) and `zoom.spec.ts` checked.

mod common;

use common::{Setup, wait_for};
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
