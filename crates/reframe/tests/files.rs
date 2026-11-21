//! Files and the clipboard: what the Electron version's `e2e/files.spec.ts` checked. A file that
//! exists only in a drag (a picture out of a browser) is written to a file and typed in as its
//! path; Ctrl+V pastes text, the paths of copied files, or a picture (written to a file, typed in
//! as its path); right-click copies the selection or, without one, pastes, unless the program
//! reads the mouse (then Shift+right-click does). The clipboard is a stand-in, so the real one is
//! never changed. Also: a web link opens on a click while the program has the mouse.
//!
//! The drag is a real one through Windows (OLE): the pointer moves over the test's window for a
//! moment.

mod common;

use common::{App, Setup, wait_for};
use serde_json::{Value, json};

/// What was typed into a session and sent, as its transcript has it.
fn prompts(setup: &Setup, claude_id: &str) -> Vec<String> {
    let file = setup
        .path("claude-home")
        .join("projects")
        .join("fake-project")
        .join(format!("{claude_id}.jsonl"));
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| entry["prompt"].as_str().map(|prompt| prompt.trim().to_owned()))
        .collect()
}

fn send(app: &mut App, session: &str) {
    std::thread::sleep(std::time::Duration::from_millis(300));
    app.write(session, "\r");
}

fn wait_prompts(setup: &Setup, claude_id: &str, count: usize) -> Vec<String> {
    wait_for(&format!("{count} prompts"), || {
        let prompts = prompts(setup, claude_id);
        (prompts.len() >= count).then_some(prompts)
    })
}

fn unquoted(text: &str) -> &str {
    text.trim_matches('"').trim_matches('\'')
}

fn cell(app: &mut App, session: &str, col: usize, row: usize) -> (f64, f64) {
    wait_for("the cell", || {
        let at = app.cmd(json!({ "cmd": "cell", "session": session, "col": col, "row": row }));
        Some((at["x"].as_f64()?, at["y"].as_f64()?))
    })
}

fn click(app: &mut App, (x, y): (f64, f64), button: &str, clicks: u64, shift: bool) {
    for kind in ["down", "up"] {
        app.cmd(json!({
            "cmd": "mouse", "type": kind, "x": x, "y": y, "button": button, "clicks": clicks, "shift": shift
        }));
    }
}

#[test]
fn dropping_files_and_pasting_text_files_and_pictures() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let alpha = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&alpha, "idle");
    let claude_id = app.record(&alpha)["claudeSessionId"].as_str().unwrap().to_owned();

    // A file that exists only in the drag (Windows: a real OLE drag; Linux drops files that
    // exist): its path when it is let go over the tile.
    #[cfg(windows)]
    {
        let (x, y) = cell(&mut app, &alpha, 30, 10);
        app.cmd(json!({ "cmd": "ole_drop", "x": x, "y": y, "files": [["note one.txt", "hello"]] }));
    }
    #[cfg(not(windows))]
    {
        let note = setup.path("note one.txt");
        std::fs::write(&note, "hello").unwrap();
        app.cmd(json!({ "cmd": "drop_files", "session": alpha, "paths": [note] }));
    }
    wait_for("the dropped path", || {
        let log = setup.log();
        assert!(!log.contains(r#""dragged":false"#), "the drag did not start: {log}");
        app.screen(&alpha).contains("note one.txt").then_some(())
    });
    send(&mut app, &alpha);
    let dropped = wait_prompts(&setup, &claude_id, 1)[0].clone();
    let dropped = unquoted(&dropped);
    assert_eq!(std::path::Path::new(dropped).file_name().unwrap(), "note one.txt");
    assert_eq!(std::fs::read_to_string(dropped).unwrap(), "hello");

    // Ctrl+V: text first, then copied files (quoted as needed), then a picture.
    let copied = if cfg!(windows) {
        r"C:\My Pictures\shot.png"
    } else {
        "/My Pictures/shot.png"
    };
    let paste = |app: &mut App, contents: Value| {
        let mut command = json!({ "cmd": "clipboard" });
        command
            .as_object_mut()
            .unwrap()
            .extend(contents.as_object().unwrap().clone());
        app.cmd(command);
        app.cmd(json!({ "cmd": "key", "keys": "ctrl-v" }));
        send(app, &alpha);
    };
    paste(&mut app, json!({ "text": "pasted words", "image": true }));
    paste(&mut app, json!({ "text": "", "files": [copied] }));
    paste(&mut app, json!({ "text": "", "image": true }));
    let pasted = wait_prompts(&setup, &claude_id, 4);
    assert_eq!(pasted[1], "pasted words");
    if cfg!(windows) {
        assert_eq!(pasted[2], format!("\"{copied}\""));
        // Saved, and pasted as its path.
        let picture = unquoted(&pasted[3]);
        assert!(picture.ends_with(".png"), "{picture}");
        assert_eq!(&std::fs::read(picture).unwrap()[1..4], b"PNG");
    } else {
        assert_eq!(pasted[2], format!("'{copied}'"));
        // Claude Code reads it from the clipboard itself, on Ctrl+V.
        assert_eq!(pasted[3], "[Image #1]");
    }

    // Right-click copies the selection (caught: the real clipboard stays as it is) and clears it,
    // so the next right-click pastes. "FAKE CLAUDE" is on the first line: a double click selects
    // its first word.
    let word = cell(&mut app, &alpha, 1, 0);
    click(&mut app, word, "left", 2, false);
    click(&mut app, word, "right", 1, false);
    let copies = wait_for("the copy", || {
        let copies = app.cmd(json!({ "cmd": "clipboard" }))["copied"].clone();
        (copies.as_array().is_some_and(|copies| !copies.is_empty())).then_some(copies)
    });
    assert_eq!(copies, json!(["FAKE"]));
    app.cmd(json!({ "cmd": "clipboard", "text": "right-clicked words" }));
    click(&mut app, word, "right", 1, false);
    send(&mut app, &alpha);
    assert_eq!(wait_prompts(&setup, &claude_id, 5)[4], "right-clicked words");

    // A program that reads the mouse (Claude Code's fullscreen mode) gets the right-click and
    // pastes on its own: reframe stays out of it. Shift+right-click is reframe's again.
    app.write(&alpha, "/mouse");
    send(&mut app, &alpha);
    app.cmd(json!({ "cmd": "clipboard", "text": "shift words" }));
    std::thread::sleep(std::time::Duration::from_millis(300));
    click(&mut app, word, "right", 1, false);
    send(&mut app, &alpha);
    assert_eq!(wait_prompts(&setup, &claude_id, 6)[5], "[right click]");
    click(&mut app, word, "right", 1, true);
    send(&mut app, &alpha);
    assert_eq!(wait_prompts(&setup, &claude_id, 7)[6], "shift words");
    app.quit();
}

#[test]
fn a_web_link_opens_on_a_click_also_while_the_program_has_the_mouse() {
    let setup = Setup::new();
    let mut app = setup.launch();
    let created = app.cmd(json!({ "cmd": "new", "cwd": setup.project(), "name": "alpha" }));
    let alpha = created["session"].as_str().unwrap().to_owned();
    app.wait_status(&alpha, "idle");
    // The fake claude repeats what it was sent: a link on screen.
    app.write(&alpha, "see https://example.com/docs");
    send(&mut app, &alpha);
    let row = wait_for("the link on screen", || {
        app.screen(&alpha)
            .lines()
            .position(|line| line.contains("https://example.com/docs") && !line.starts_with('>'))
    });
    let line = app.screen(&alpha).lines().nth(row).unwrap().to_owned();
    let col = line.find("https://").unwrap() + 4;
    // The program reads the mouse.
    app.write(&alpha, "/mouse");
    send(&mut app, &alpha);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let at = cell(&mut app, &alpha, col, row);
    app.cmd(json!({ "cmd": "mouse", "type": "move", "x": at.0, "y": at.1 }));
    click(&mut app, at, "left", 1, false);
    let links = wait_for("the link to open", || {
        let links = app.cmd(json!({ "cmd": "links" }));
        (links.as_array().is_some_and(|links| !links.is_empty())).then_some(links)
    });
    assert_eq!(links, json!(["https://example.com/docs"]));
    app.quit();
}
