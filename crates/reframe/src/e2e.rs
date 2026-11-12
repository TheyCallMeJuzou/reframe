//! How tests drive the app (`REFRAME_E2E`): commands as JSON lines over a local socket, each
//! answered with one JSON line. The port is written to `e2e-port` in the profile once the app is
//! ready. Without `REFRAME_E2E` none of this runs.
//!
//! It stands in for what the Electron version's tests did through Playwright: reading the app's
//! state and acting as the user would.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::sync::mpsc::{Sender, channel};
use std::thread;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::*;
use serde_json::{Value, json};

use reframe_core::actions::LayoutAction;
use reframe_core::geometry::Rect;

use crate::app::{Controller, NewSessionRequest};
use crate::sessions::status_name;
use crate::window::{self, Shell};

type Command = (Value, Sender<Value>);

/// Starts listening and answering. Call when the app is ready for commands.
pub fn start(controller: Entity<Controller>, cx: &mut App) {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
        Ok(listener) => listener,
        Err(error) => return eprintln!("e2e: could not listen: {error}"),
    };
    let port = listener.local_addr().map(|address| address.port()).unwrap_or(0);
    let (sender, mut commands) = unbounded::<Command>();
    let _ = thread::Builder::new()
        .name("reframe-e2e".into())
        .spawn(move || accept(&listener, &sender));

    let file = controller.read(cx).config().user_data.join("e2e-port");
    cx.spawn(async move |cx| {
        while let Some((command, reply)) = commands.next().await {
            let answer = cx.update(|cx| run(&controller, &command, cx));
            let _ = reply.send(answer);
        }
    })
    .detach();

    if let Err(error) = std::fs::write(&file, port.to_string()) {
        eprintln!("e2e: could not write {}: {error}", file.display());
    }
}

fn accept(listener: &TcpListener, sender: &UnboundedSender<Command>) {
    for stream in listener.incoming().flatten() {
        let sender = sender.clone();
        let _ = thread::Builder::new().name("reframe-e2e-client".into()).spawn(move || {
            let Ok(mut out) = stream.try_clone() else { return };
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let answer = match serde_json::from_str::<Value>(&line) {
                    Ok(command) => {
                        let (reply, answered) = channel();
                        if sender.unbounded_send((command, reply)).is_err() {
                            break;
                        }
                        // No answer: the app is gone.
                        let Ok(answer) = answered.recv() else { break };
                        answer
                    }
                    Err(error) => json!({ "error": format!("not JSON: {error}") }),
                };
                if writeln!(out, "{answer}").is_err() {
                    break;
                }
            }
        });
    }
}

fn run(controller: &Entity<Controller>, command: &Value, cx: &mut App) -> Value {
    let text = |name: &str| command.get(name).and_then(Value::as_str).map(str::to_owned);
    let flag = |name: &str| command.get(name).and_then(Value::as_bool).unwrap_or(false);
    let session = text("session").unwrap_or_default();
    match text("cmd").as_deref() {
        // The state as it would be saved now.
        Some("state") => serde_json::from_str(&controller.read(cx).state().to_json()).unwrap_or(Value::Null),
        // What the tiles show of each session with a process.
        Some("sessions") => {
            let sessions = controller.read(cx).sessions().read(cx);
            let mut list = serde_json::Map::new();
            for id in sessions.active() {
                let view = json!({
                    "status": sessions.status(&id).map(status_name),
                    "error": sessions.error(&id).map(|error| error.key()),
                    "pid": sessions.pid(&id),
                    "claudeName": sessions.registry_name(&id),
                });
                list.insert(id, view);
            }
            Value::Object(list)
        }
        Some("new") => {
            let request = NewSessionRequest {
                cwd: text("cwd").unwrap_or_default(),
                name: text("name"),
                model: text("model"),
                worktree: flag("worktree"),
                worktree_name: text("worktreeName"),
                ..NewSessionRequest::default()
            };
            controller.update(cx, |controller, cx| {
                let window =
                    text("window").or_else(|| controller.state().windows.first().map(|window| window.id.clone()));
                match controller.create_session(&window.unwrap_or_default(), request, cx) {
                    Ok(id) => json!({ "ok": true, "session": id }),
                    Err(error) => json!({ "ok": false, "error": error.key() }),
                }
            })
        }
        // Types into a session, as keys would.
        Some("write") => {
            let terminal = controller.read(cx).sessions().read(cx).terminal(&session);
            match terminal {
                Some(terminal) => {
                    terminal
                        .read(cx)
                        .pty()
                        .write(text("text").unwrap_or_default().into_bytes());
                    json!({ "ok": true })
                }
                None => json!({ "ok": false }),
            }
        }
        Some("screen") => {
            let terminal = controller.read(cx).sessions().read(cx).terminal(&session);
            json!({ "text": terminal.map(|terminal| terminal.read(cx).pty().screen_text()) })
        }
        Some("restart") => {
            controller.update(cx, |controller, cx| {
                controller.restart_session(&session, flag("fresh"), cx)
            });
            json!({ "ok": true })
        }
        Some("end") => {
            controller.update(cx, |controller, cx| controller.end_session(&session, cx));
            json!({ "ok": true })
        }
        Some("remove") => {
            controller.update(cx, |controller, cx| controller.remove_session(&session, cx));
            json!({ "ok": true })
        }
        // A change to the first window's layout, as its tiles and its strip ask for it.
        Some("layout") => {
            let action = match text("action").as_deref() {
                Some("collapse") => LayoutAction::TileCollapse { session_id: session },
                Some("expand") => LayoutAction::TileExpand { session_id: session },
                _ => return json!({ "error": "unknown layout action" }),
            };
            controller.update(cx, |controller, cx| {
                let window = controller.state().windows[0].id.clone();
                controller.dispatch(&window, &action, cx);
            });
            json!({ "ok": true })
        }
        Some("zoom") => match first_shell(controller, cx) {
            Some(shell) => {
                shell.update(cx, |shell, cx| shell.toggle_zoom(&session, cx));
                json!({ "ok": true })
            }
            None => json!({ "ok": false }),
        },
        // Where the first window drew its tiles and separators in its last frame.
        Some("placement") => match first_shell(controller, cx) {
            Some(shell) => {
                let shell = shell.read(cx);
                let rect = |rect: &Rect| json!([rect.x, rect.y, rect.width, rect.height]);
                let placement = shell.placement();
                json!({
                    "area": rect(&shell.area()),
                    "zoomed": shell.zoomed(),
                    "tiles": placement.tiles.iter().map(|tile| json!({ "session": tile.session_id, "rect": rect(&tile.rect) })).collect::<Vec<_>>(),
                    "separators": placement.separators.iter().map(|separator| json!({ "split": separator.split_id, "index": separator.index, "rect": rect(&separator.rect) })).collect::<Vec<_>>(),
                })
            }
            None => Value::Null,
        },
        // Drags a separator by so many pixels ("free": with Alt held, off the grid).
        Some("drag") => match first_shell(controller, cx) {
            Some(shell) => {
                let split = text("split").unwrap_or_default();
                let index = command.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = command.get("delta").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                shell.update(cx, |shell, cx| {
                    let area = shell.area();
                    shell.drag_separator(&split, index, delta, flag("free"), area, cx);
                });
                json!({ "ok": true })
            }
            None => json!({ "ok": false }),
        },
        Some("quit") => {
            controller.update(cx, |controller, cx| controller.shutdown(cx));
            json!({ "ok": true })
        }
        other => json!({ "error": format!("unknown command: {other:?}") }),
    }
}

fn first_shell(controller: &Entity<Controller>, cx: &App) -> Option<Entity<Shell>> {
    let id = controller.read(cx).state().windows.first()?.id.clone();
    window::shell(&id, cx)
}
