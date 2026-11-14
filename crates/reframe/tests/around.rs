//! Around the windows with the fake claude: sessions that need the user, the tray and its menu,
//! windows that stay hidden, a start by the system, a second start, and the system going down.
//! What the Electron version's `e2e/needs-you.spec.ts`, `autostart.spec.ts`,
//! `system-shutdown.spec.ts` and the tray parts of `closed-windows.spec.ts` checked.

mod common;

use std::process::Command;

use common::{App, Setup, wait_for};
use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_QUERYENDSESSION};

fn new_session(app: &mut App, setup: &Setup, name: &str) -> String {
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": name }));
    created["session"].as_str().unwrap().to_owned()
}

fn window(app: &mut App) -> Value {
    app.cmd(json!({ "cmd": "window" }))
}

fn key(app: &mut App, keys: &str) {
    let answer = app.cmd(json!({ "cmd": "key", "keys": keys }));
    assert_eq!(answer["handled"], true, "{keys} was not taken");
}

fn focused(app: &mut App, session: &str, what: &str) {
    wait_for(what, || (window(app)["focused"] == session).then_some(()));
}

fn title(app: &mut App, title: &str) {
    wait_for(&format!("the title {title:?}"), || {
        (window(app)["title"] == title).then_some(())
    });
}

fn starts(setup: &Setup, name: &str) -> Vec<Value> {
    let log = setup.fake_log();
    log.into_iter()
        .filter(|entry| entry["event"] == "start" && common::args(entry).contains(&name))
        .collect()
}

/// The windows as Windows shows them, in the state's order.
fn visible(app: &mut App) -> Vec<bool> {
    app.cmd(json!({ "cmd": "windows" }))
        .as_array()
        .unwrap()
        .iter()
        .map(|window| window["visible"] == true)
        .collect()
}

#[test]
fn the_title_counts_the_sessions_that_need_you_and_a_shortcut_goes_to_the_next() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    let beta = new_session(&mut app, &setup, "beta");
    for id in [&alpha, &beta] {
        app.wait_status(id, "idle");
    }
    wait_for("the window to take keys", || {
        (window(&mut app)["keyboard"] == true).then_some(())
    });
    title(&mut app, "reframe");
    let ask = |app: &mut App, id: &str| {
        app.write(id, "/ask\r");
        app.wait_status(id, "waiting");
    };

    ask(&mut app, &beta);
    title(&mut app, "reframe – a session needs you");
    wait_for("the tray to say so", || {
        (app.cmd(json!({ "cmd": "tray" }))["tooltip"] == "reframe – a session needs you").then_some(())
    });
    // From alpha, which needs nothing, the shortcut goes to beta; answered there, nobody needs
    // the user any more.
    key(&mut app, "ctrl-1");
    focused(&mut app, &alpha, "alpha to have the keyboard");
    key(&mut app, "ctrl-shift-space");
    focused(&mut app, &beta, "the shortcut to reach beta");
    app.write(&beta, "\r");
    app.wait_status(&beta, "idle");
    title(&mut app, "reframe");
    assert_eq!(app.cmd(json!({ "cmd": "tray" }))["tooltip"], "reframe");

    // Both need the user, one of them collapsed: the shortcut brings that one back, and goes
    // round between them.
    ask(&mut app, &alpha);
    ask(&mut app, &beta);
    title(&mut app, "reframe – 2 sessions need you");
    app.cmd(json!({ "cmd": "layout", "action": "collapse", "session": alpha }));
    key(&mut app, "ctrl-2");
    key(&mut app, "ctrl-1");
    focused(&mut app, &beta, "beta, the only tile, to have the keyboard");
    key(&mut app, "ctrl-shift-space");
    wait_for("alpha to come back from the strip", || {
        (app.state()["windows"][0]["dock"] == json!([])).then_some(())
    });
    focused(&mut app, &alpha, "the shortcut to reach alpha");
    key(&mut app, "ctrl-shift-space");
    focused(&mut app, &beta, "the shortcut to go on to beta");

    // Notifications are off in tests, which the log says for each one.
    assert!(setup.log().contains("notification skipped"));
    app.quit();
}

#[test]
fn the_tray_shows_and_hides_windows_and_is_the_way_to_quit() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    let beta = new_session(&mut app, &setup, "beta");
    for id in [&alpha, &beta] {
        app.wait_status(id, "idle");
    }
    let labels = |app: &mut App| -> Vec<Value> {
        let tray = app.cmd(json!({ "cmd": "tray" }));
        tray["menu"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["label"].clone())
            .collect()
    };
    let click = |app: &mut App, label: &str| {
        assert_eq!(
            app.cmd(json!({ "cmd": "tray_click", "label": label }))["ok"],
            true,
            "{label}"
        );
    };
    let first = "Window 1 – alpha, beta";
    wait_for("the tray menu to name the sessions", || {
        (labels(&mut app)
            == [
                json!(first),
                Value::Null,
                json!("New session"),
                json!("New window"),
                Value::Null,
                json!("About reframe"),
                json!("Quit reframe"),
            ])
        .then_some(())
    });
    assert_eq!(app.cmd(json!({ "cmd": "tray" }))["menu"][0]["checked"], true);
    wait_for("the window to be on screen", || {
        (visible(&mut app) == [true]).then_some(())
    });

    // A window's entry hides it and shows it again; hidden is remembered.
    click(&mut app, first);
    wait_for("the window to be hidden", || {
        (visible(&mut app) == [false]).then_some(())
    });
    wait_for("that to be remembered", || {
        (app.state()["windows"][0]["hidden"] == true).then_some(())
    });
    assert_eq!(app.cmd(json!({ "cmd": "tray" }))["menu"][0]["checked"], false);
    assert_eq!(app.status(&alpha), "idle");
    click(&mut app, first);
    wait_for("the window to be back", || (visible(&mut app) == [true]).then_some(()));
    assert_eq!(app.state()["windows"][0]["hidden"], false);

    // Closed with its sessions kept, the window waits in the tray and reframe runs on; a click
    // on the icon brings it back.
    app.cmd(json!({ "cmd": "close" }));
    app.cmd(json!({ "cmd": "close_answer", "choice": "keep" }));
    wait_for("the closed window to be hidden", || {
        (visible(&mut app) == [false]).then_some(())
    });
    assert_eq!(app.status(&beta), "idle");
    assert_eq!(app.cmd(json!({ "cmd": "tray_click", "icon": true }))["ok"], true);
    wait_for("the window to be back", || (visible(&mut app) == [true]).then_some(()));

    // "New window", and "New session" which opens the dialog in the first window, shown.
    click(&mut app, "New window");
    let second = wait_for("the second window", || {
        app.state()["windows"][1]["id"].as_str().map(str::to_owned)
    });
    wait_for("its entry in the menu", || {
        (labels(&mut app)[1] == "Window 2").then_some(())
    });
    click(&mut app, first);
    wait_for("the first window to be hidden", || {
        (visible(&mut app) == [false, true]).then_some(())
    });
    click(&mut app, "New session");
    wait_for("the dialog in the first window, which is back", || {
        (visible(&mut app) == [true, true] && window(&mut app)["dialog"] == true).then_some(())
    });
    app.cmd(json!({ "cmd": "key", "keys": "escape" }));

    // An empty second window that is closed is gone, as before; the first one only hides.
    app.cmd(json!({ "cmd": "close", "window": second }));
    wait_for("the empty window to go", || {
        (app.cmd(json!({ "cmd": "windows" })).as_array().unwrap().len() == 1).then_some(())
    });

    // "Quit reframe" ends the sessions (they stay open) and the process.
    click(&mut app, "Quit reframe");
    let child = &mut app.child;
    let status = wait_for("reframe to quit", || child.try_wait().unwrap());
    assert!(status.success());
    assert_eq!(setup.saved_state()["sessions"][&alpha]["lifecycle"], "open");
}

#[test]
fn started_by_the_system_windows_stay_hidden_and_a_second_start_shows_them() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    app.wait_status(&alpha, "idle");
    app.quit();

    // Started by the system: no window shows, the session resumes all the same.
    let mut app = setup.launch_with(&["--autostart"]);
    app.wait_status(&alpha, "idle");
    assert_eq!(visible(&mut app), [false]);
    assert_eq!(starts(&setup, "alpha").len(), 2);
    assert_eq!(app.state()["windows"][0]["hidden"], false, "not closed by the user");

    // Started again, as from the Start menu: that process leaves, and the window is there with
    // its session.
    let again = setup.command().status().expect("the second start");
    assert!(again.success());
    wait_for("the window to show", || (visible(&mut app) == [true]).then_some(()));
    wait_for("the window to be drawn", || {
        let placed = app.cmd(json!({ "cmd": "placement" }));
        (placed["tiles"].as_array().is_some_and(|tiles| tiles.len() == 1)).then_some(())
    });
    assert!(setup.log().contains("reframe already runs, showing its windows"));
    assert_eq!(starts(&setup, "alpha").len(), 2);
    assert_eq!(app.status(&alpha), "idle");

    // A window the user closed stays closed when reframe is started again; with every window
    // closed, the first one shows.
    let second = app.cmd(json!({ "cmd": "new_window" }))["window"]
        .as_str()
        .unwrap()
        .to_owned();
    wait_for("the second window", || {
        (visible(&mut app) == [true, true]).then_some(())
    });
    let first = app.state()["windows"][0]["id"].clone();
    app.cmd(json!({ "cmd": "transfer", "session": alpha, "from": first, "to": second }));
    app.cmd(json!({ "cmd": "close", "window": second }));
    app.cmd(json!({ "cmd": "close_answer", "window": second, "choice": "keep" }));
    wait_for("the second window to be hidden", || {
        (visible(&mut app) == [true, false]).then_some(())
    });
    assert!(setup.command().status().unwrap().success());
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(visible(&mut app), [true, false]);
    app.cmd(json!({ "cmd": "close" }));
    wait_for("both to be hidden", || {
        (visible(&mut app) == [false, false]).then_some(())
    });
    assert!(setup.command().status().unwrap().success());
    wait_for("the first window to show", || {
        (visible(&mut app) == [true, false]).then_some(())
    });
    app.quit();
}

#[test]
fn sessions_the_system_ends_on_its_way_down_resume_on_the_next_start() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let alpha = new_session(&mut app, &setup, "alpha");
    let beta = new_session(&mut app, &setup, "beta");
    for id in [&alpha, &beta] {
        app.wait_status(id, "idle");
    }
    let end = |app: &mut App, name: &str, id: &str| {
        let pid = starts(&setup, name)[0]["pid"].as_u64().unwrap();
        let killed = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .unwrap();
        assert!(killed.status.success());
        // Ended, or failed to start (this soon after starting): either way it has no process.
        wait_for("the session to be without its claude", || {
            let status = app.status(id);
            (status != "idle" && status != "starting").then_some(())
        });
    };

    // alpha's claude ends while the system runs on: it stays ended.
    end(&mut app, "alpha", &alpha);
    wait_for("alpha to count as ended", || {
        (app.record(&alpha)["lifecycle"] == "ended").then_some(())
    });

    // Windows asks whether it may go down, then ends beta's claude: beta stays open.
    let control = app.cmd(json!({ "cmd": "control" }))["hwnd"].as_i64().unwrap();
    // SAFETY: a message to a window of the app under test.
    unsafe {
        SendMessageW(
            HWND(control as isize as *mut core::ffi::c_void),
            WM_QUERYENDSESSION,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        );
    }
    wait_for("reframe to hear of it", || {
        setup
            .log()
            .contains("the system is going down, sessions it ends stay open")
            .then_some(())
    });
    end(&mut app, "beta", &beta);
    assert_eq!(app.record(&beta)["lifecycle"], "open");
    // Nothing is asked while the system goes down: the window just goes out of sight.
    app.cmd(json!({ "cmd": "close" }));
    assert_eq!(window(&mut app)["asking"], false);
    app.quit();

    let mut app = setup.launch();
    app.wait_status(&beta, "idle");
    assert_eq!(app.record(&alpha)["lifecycle"], "ended");
    assert_eq!(starts(&setup, "beta").len(), 2);
    assert_eq!(starts(&setup, "alpha").len(), 1);
    app.quit();
}
