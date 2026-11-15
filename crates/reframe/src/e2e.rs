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
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;
use serde_json::{Value, json};

use reframe_core::actions::LayoutAction;
use reframe_core::geometry::Rect;
use reframe_core::layout::{DropTarget, find_node, tile_key};
use reframe_core::session_list::SessionList;
use reframe_core::state::{CloseWindow, Side};

use crate::app::{Controller, NewSessionRequest};
use crate::sessions::status_name;
use crate::tray;
use crate::win::{self, TrayEntry, TrayEvent};
use crate::window::{self, DragKind, MenuEntry, RenameTarget, Shell};

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
    let number = |name: &str| command.get(name).and_then(Value::as_f64).unwrap_or(0.0);
    let session = text("session").unwrap_or_default();
    // A tile (by its session) or a group, as the layout names them.
    let key = text("key")
        .or_else(|| text("group"))
        .unwrap_or_else(|| tile_key(&session));
    let kind = if key.starts_with("tile:") {
        DragKind::Tile
    } else {
        DragKind::Group
    };
    let target = command
        .get("target")
        .filter(|target| target.is_object())
        .and_then(|target| {
            Some(DropTarget {
                key: target.get("key")?.as_str()?.to_owned(),
                side: serde_json::from_value::<Side>(target.get("side")?.clone()).ok()?,
            })
        });
    let rect = |rect: &Rect| json!([rect.x, rect.y, rect.width, rect.height]);
    match text("cmd").as_deref() {
        // The open windows, in the state's order.
        Some("windows") => {
            let ids: Vec<String> = controller
                .read(cx)
                .state()
                .windows
                .iter()
                .map(|window| window.id.clone())
                .collect();
            let windows: Vec<Value> = ids
                .iter()
                .map(|id| {
                    let shell = window::shell(id, cx);
                    json!({
                        "id": id,
                        "open": shell.is_some(),
                        "away": shell.as_ref().is_some_and(|shell| shell.read(cx).is_away()),
                        // Whether Windows shows it.
                        "visible": shell.is_some_and(|shell| shell.read(cx).is_on_screen()),
                    })
                })
                .collect();
            json!(windows)
        }
        // The tray icon's tooltip and menu; a separator is null.
        Some("tray") => {
            let (tooltip, menu) = win::tray();
            let menu: Vec<Value> = menu.iter().map(tray_json).collect();
            json!({ "tooltip": tooltip, "menu": menu })
        }
        // A click on the tray icon ("icon") or on the entry of its menu with that label.
        Some("tray_click") => {
            let event = if flag("icon") {
                Some(TrayEvent::Clicked)
            } else {
                let label = text("label").unwrap_or_default();
                tray_command(&win::tray().1, &label).map(TrayEvent::Command)
            };
            match event {
                Some(event) => {
                    tray::handle(controller, event, cx);
                    json!({ "ok": true })
                }
                None => json!({ "ok": false }),
            }
        }
        // The window that takes what Windows tells reframe (a second start, the system going
        // down).
        Some("control") => json!({ "hwnd": tray::control_window(cx) }),
        Some("new_window") => match shell_of(controller, text("window"), cx) {
            Some(shell) => json!({ "window": shell.update(cx, |shell, cx| shell.new_window(cx)) }),
            None => json!({ "error": "no window" }),
        },
        // What dropping a tile or group from another window does: it moves over.
        Some("transfer") => {
            let (from, to) = (text("from").unwrap_or_default(), text("to").unwrap_or_default());
            let moved = controller.update(cx, |controller, cx| {
                controller.transfer_node(&to, &key, &from, target.as_ref(), cx)
            });
            json!({ "ok": moved })
        }
        // What letting a tile or group go at a point of the screen does: a window of its own,
        // unless one of reframe's windows is there.
        Some("tear_out") => {
            let point = point(px(number("x") as f32), px(number("y") as f32));
            if window::window_at_screen(point, None, cx).is_some() {
                return json!({ "ok": false });
            }
            let torn = controller.update(cx, |controller, cx| {
                let from = controller
                    .state()
                    .windows
                    .iter()
                    .find(|window| find_node(window.layout.as_ref(), &key).is_some())
                    .map(|window| window.id.clone())?;
                controller.tear_out(&from, &key, (number("x"), number("y")), None, cx)
            });
            json!({ "ok": torn.is_some(), "window": torn })
        }
        // Lets a tile or group of a window go at a point of a window ("to"), as the pointer
        // would.
        Some("drop") => match shell_of(controller, text("window"), cx) {
            Some(shell) => {
                let to = text("to").unwrap_or_else(|| shell.read(cx).window_id().to_owned());
                let at = point(px(number("x") as f32), px(number("y") as f32));
                let result = shell.update(cx, |shell, cx| {
                    shell.drop_node(&key, kind, Some((to, at)), at, None, cx)
                });
                json!({ "result": result })
            }
            None => json!({ "error": "no window" }),
        },
        // Where a tile or group dragged over a point of a window would go.
        Some("spot") => match shell_of(controller, text("window"), cx) {
            Some(shell) => {
                let at = point(px(number("x") as f32), px(number("y") as f32));
                let (target, preview) = shell.read(cx).drop_spot_at(at, &key, kind, cx);
                json!({
                    "target": target.map(|target| json!({ "key": target.key, "side": target.side })),
                    "preview": preview.as_ref().map(rect),
                })
            }
            None => json!({ "error": "no window" }),
        },
        // What the tiles' and groups' menus do.
        Some("group") => {
            let group = text("group").unwrap_or_default();
            let color = text("color").unwrap_or_default();
            controller.update(cx, |controller, cx| match text("action").as_deref() {
                Some("create") => controller.create_group(&session, cx),
                Some("add") => controller.add_session_to_group(&session, &group, cx),
                Some("remove") => controller.remove_session_from_group(&session, cx),
                Some("dissolve") => controller.dissolve_group(&group, cx),
                Some("recolor") => controller.recolor_group(&group, &color, cx),
                Some("endAll") => controller.end_group(&group, cx),
                _ => {}
            });
            json!({ "ok": true })
        }
        // Opens a tile's menu ("session"), a group's ("group") or its colours ("colors").
        Some("menu") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| {
                let entries = match text("group") {
                    Some(group) => shell.group_menu(&group, flag("colors"), cx),
                    None => shell.tile_menu(&session, cx),
                };
                let labels = menu_json(&entries);
                shell.show_menu(entries, point(px(40.0), px(80.0)), window, cx);
                json!({ "menu": labels })
            })
        }),
        Some("menu_click") => in_window(controller, text("window"), cx, |shell, window, cx| {
            let label = text("label").unwrap_or_default();
            json!({ "ok": shell.update(cx, |shell, cx| shell.click_menu_item(&label, window, cx)) })
        }),
        // A double click on a group's name: the field to rename it in.
        Some("rename_group") => in_window(controller, text("window"), cx, |shell, window, cx| {
            let group = text("group").unwrap_or_default();
            shell.update(cx, |shell, cx| shell.start_group_rename(group, window, cx));
            json!({ "ok": true })
        }),
        // The window's X.
        Some("close") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| shell.request_close(window, cx));
            json!({ "ok": true })
        }),
        // The answer to the question the X asks: "end", "keep", or nothing for Cancel.
        Some("close_answer") => in_window(controller, text("window"), cx, |shell, window, cx| {
            let choice = match text("choice").as_deref() {
                Some("end") => Some(CloseWindow::End),
                Some("keep") => Some(CloseWindow::Keep),
                _ => None,
            };
            shell.update(cx, |shell, cx| shell.answer_close(choice, flag("remember"), window, cx));
            json!({ "ok": true })
        }),
        // The pointer in a window: "down", "move" (with the left button held) and "up" at a
        // point, as the system would report them. "button": "right" for the other one.
        Some("mouse") => in_window(controller, text("window"), cx, |_, window, cx| {
            let position = point(px(number("x") as f32), px(number("y") as f32));
            let button = if text("button").as_deref() == Some("right") {
                MouseButton::Right
            } else {
                MouseButton::Left
            };
            let modifiers = Modifiers::default();
            let click_count = command.get("clicks").and_then(Value::as_u64).unwrap_or(1) as usize;
            let event = match text("type").as_deref() {
                Some("down") => PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count,
                    first_mouse: false,
                }),
                Some("up") => PlatformInput::MouseUp(MouseUpEvent {
                    button,
                    position,
                    modifiers,
                    click_count,
                }),
                _ => PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: flag("held").then_some(button),
                    modifiers,
                }),
            };
            window.dispatch_event(event, cx);
            json!({ "ok": true })
        }),
        // Files dragged from outside over a window: "enter" with their paths, "move", "drop",
        // or "exit".
        Some("file_drag") => in_window(controller, text("window"), cx, |_, window, cx| {
            let position = point(px(number("x") as f32), px(number("y") as f32));
            let paths: Vec<std::path::PathBuf> = command
                .get("paths")
                .and_then(Value::as_array)
                .map(|paths| paths.iter().filter_map(Value::as_str).map(Into::into).collect())
                .unwrap_or_default();
            let event = match text("type").as_deref() {
                Some("enter") => FileDropEvent::Entered {
                    position,
                    paths: ExternalPaths(paths.into_iter().collect()),
                },
                Some("drop") => FileDropEvent::Submit { position },
                Some("exit") => FileDropEvent::Exited,
                _ => FileDropEvent::Pending { position },
            };
            window.dispatch_event(PlatformInput::FileDrop(event), cx);
            json!({ "ok": true })
        }),
        // Files dropped onto a session's tile.
        Some("drop_files") => match shell_of(controller, text("window"), cx) {
            Some(shell) => {
                let paths: Vec<std::path::PathBuf> = command
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|paths| paths.iter().filter_map(Value::as_str).map(Into::into).collect())
                    .unwrap_or_default();
                shell.update(cx, |shell, cx| shell.drop_files(&session, &paths, cx));
                json!({ "ok": true })
            }
            None => json!({ "error": "no window" }),
        },
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
        // A change to a window's layout, as its tiles, groups and its strip ask for it.
        Some("layout") => {
            let group_id = text("group").unwrap_or_default();
            let action = match text("action").as_deref() {
                Some("collapse") => LayoutAction::TileCollapse { session_id: session },
                Some("expand") => LayoutAction::TileExpand { session_id: session },
                Some("groupCollapse") => LayoutAction::GroupCollapse { group_id },
                Some("groupExpand") => LayoutAction::GroupExpand { group_id },
                _ => return json!({ "error": "unknown layout action" }),
            };
            controller.update(cx, |controller, cx| {
                let window = text("window").unwrap_or_else(|| controller.state().windows[0].id.clone());
                controller.dispatch(&window, &action, cx);
            });
            json!({ "ok": true })
        }
        Some("zoom") => match shell_of(controller, text("window"), cx) {
            Some(shell) => {
                shell.update(cx, |shell, cx| shell.toggle_zoom(&session, cx));
                json!({ "ok": true })
            }
            None => json!({ "ok": false }),
        },
        // Where the first window drew its tiles and separators in its last frame.
        Some("placement") => match shell_of(controller, text("window"), cx) {
            Some(shell) => {
                let shell = shell.read(cx);
                let placement = shell.placement();
                json!({
                    "area": rect(&shell.area()),
                    "zoomed": shell.zoomed(),
                    "tiles": placement.tiles.iter().map(|tile| json!({ "session": tile.session_id, "rect": rect(&tile.rect), "group": tile.group })).collect::<Vec<_>>(),
                    "groups": placement.groups.iter().map(|group| json!({ "id": group.id, "rect": rect(&group.rect) })).collect::<Vec<_>>(),
                    "separators": placement.separators.iter().map(|separator| json!({ "split": separator.split_id, "index": separator.index, "rect": rect(&separator.rect) })).collect::<Vec<_>>(),
                })
            }
            None => Value::Null,
        },
        // Drags a separator by so many pixels ("free": with Alt held, off the grid).
        Some("drag") => match shell_of(controller, text("window"), cx) {
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
        // A key pressed in the first window, written as GPUI does ("ctrl-shift-t", "enter").
        Some("key") => in_window(controller, text("window"), cx, |_, window, cx| {
            // Kept for good: a handful of short strings in a test run.
            let keys: &'static str = Box::leak(text("keys").unwrap_or_default().into_boxed_str());
            match Keystroke::parse(keys) {
                Ok(keystroke) => json!({ "handled": window.dispatch_keystroke(keystroke, cx) }),
                Err(error) => json!({ "error": error.to_string() }),
            }
        }),
        // Text typed into whatever has the keyboard in the first window.
        Some("type") => in_window(controller, text("window"), cx, |_, window, cx| {
            for character in text("text").unwrap_or_default().chars() {
                let keystroke = Keystroke {
                    modifiers: Modifiers::default(),
                    key: character.to_string(),
                    key_char: Some(character.to_string()),
                };
                window.dispatch_keystroke(keystroke, cx);
            }
            json!({ "ok": true })
        }),
        Some("open") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| match text("dialog").as_deref() {
                Some("settings") => shell.open_settings(window, cx),
                Some("about") => shell.open_about(window, cx),
                _ => shell.open_new_session(window, cx),
            });
            json!({ "ok": true })
        }),
        // A double click on a tile's name: the field to rename it in.
        Some("rename") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| shell.start_rename(session.clone(), window, cx));
            json!({ "ok": true })
        }),
        // The first window made this big, as dragging its edge would.
        Some("resize") => in_window(controller, text("window"), cx, |_, window, _| {
            let side = |name: &str| px(command.get(name).and_then(Value::as_f64).unwrap_or(800.0) as f32);
            window.resize(size(side("width"), side("height")));
            json!({ "ok": true })
        }),
        // What the first window shows beyond its layout.
        Some("window") => in_window(controller, text("window"), cx, |shell, window, cx| {
            let renaming = match shell.read(cx).renaming() {
                Some(RenameTarget::Session(id)) => (Some(id.clone()), None),
                Some(RenameTarget::Group(id)) => (None, Some(id.clone())),
                _ => (None, None),
            };
            let renaming_list = match shell.read(cx).renaming() {
                Some(RenameTarget::ListSession(id)) => json!({ "session": id }),
                Some(RenameTarget::Workspace(id)) => json!({ "workspace": id }),
                _ => Value::Null,
            };
            let question = shell.read(cx).question().map(|question| {
                json!({
                    "title": question.title,
                    "message": question.message,
                    "confirm": question.confirm,
                    "cancel": question.cancel,
                })
            });
            let screen = window.bounds();
            json!({
                "dialog": window.has_active_dialog(cx),
                "focused": shell.read(cx).active_tile(window, cx),
                "title": shell.read(cx).title(cx),
                "fullscreen": window.is_fullscreen(),
                "drawn": shell.read(cx).drawn() > 0,
                "frames": shell.read(cx).drawn(),
                // The window or one of its tiles has the keyboard, so keys reach reframe's shortcuts.
                "keyboard": shell.read(cx).has_keyboard(window, cx),
                "size": [f32::from(window.bounds().size.width), f32::from(window.bounds().size.height)],
                "renaming": renaming.0,
                "renamingGroup": renaming.1,
                "renamingInList": renaming_list,
                "question": question,
                "sidebar": shell.read(cx).sidebar_open(cx),
                "selected": shell.read(cx).selected_rows(),
                "newFound": shell.read(cx).new_found(cx),
                "menu": shell.read(cx).menu().map(menu_json),
                "asking": shell.read(cx).asking_to_close(),
                "away": shell.read(cx).is_away(),
                "preview": shell.read(cx).drop_preview().as_ref().map(rect),
                "dragging": shell.read(cx).dragging(),
                "filesOver": shell.read(cx).files_over(),
                "screen": [f32::from(screen.origin.x), f32::from(screen.origin.y), f32::from(screen.size.width), f32::from(screen.size.height)],
                "find": shell.read(cx).find_state(cx).map(|(session, text, result)| json!({
                    "session": session,
                    "text": text,
                    "index": result.and_then(|result| result.index),
                    "count": result.map(|result| result.count),
                })),
            })
        }),
        // The session list as a window shows it.
        Some("list") => match shell_of(controller, text("window"), cx) {
            Some(shell) => shell.read(cx).shown_list().map_or(Value::Null, |list| list_json(&list)),
            None => json!({ "error": "no window" }),
        },
        // Shows the session list or takes it away (Ctrl+L).
        Some("sidebar") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| shell.toggle_sidebar(window, cx));
            json!({ "ok": true })
        }),
        // Types into the list's search box.
        Some("search") => in_window(controller, text("window"), cx, |shell, window, cx| {
            let value = text("text").unwrap_or_default();
            shell.update(cx, |shell, cx| shell.set_search(value, window, cx));
            json!({ "ok": true })
        }),
        // A click on a list row (by its key, as "app:<id>" or "claude:<id>").
        Some("row_click") => in_window(controller, text("window"), cx, |shell, _, cx| {
            let row = shell.read(cx).row_of(&text("row").unwrap_or_default());
            match row {
                Some(row) => {
                    shell.update(cx, |shell, cx| shell.click_row(&row, flag("shift"), flag("ctrl"), cx));
                    json!({ "ok": true })
                }
                None => json!({ "ok": false }),
            }
        }),
        // A right-click on a list row: its menu opens (see "menu_click").
        Some("row_menu") => in_window(controller, text("window"), cx, |shell, window, cx| {
            let row = shell.read(cx).row_of(&text("row").unwrap_or_default());
            let Some(row) = row else { return json!({ "ok": false }) };
            shell.update(cx, |shell, cx| {
                let entries = shell.row_menu(&row, cx);
                shell.show_menu(entries, point(px(40.0), px(80.0)), window, cx);
            });
            json!({ "ok": true, "menu": shell.read(cx).menu().map(menu_json) })
        }),
        // The toolbar's Workspaces menu opens (see "menu_click").
        Some("workspace_menu") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| {
                let entries = shell.workspace_menu(cx);
                shell.show_menu(entries, point(px(320.0), px(36.0)), window, cx);
            });
            json!({ "ok": true, "menu": shell.read(cx).menu().map(menu_json) })
        }),
        // List rows let go at a point of a window (`at`, by default the window itself).
        Some("row_drop") => match shell_of(controller, text("window"), cx) {
            Some(shell) => {
                let keys: Vec<String> = command
                    .get("rows")
                    .and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                let refs: Vec<_> = keys.iter().filter_map(|key| shell.read(cx).row_of(key)).collect();
                let at = text("at").or_else(|| Some(shell.read(cx).window_id().to_owned()));
                let place = at.map(|id| (id, point(px(number("x") as f32), px(number("y") as f32))));
                let result = shell.update(cx, |shell, cx| shell.drop_rows(refs, place, cx));
                json!({ "result": result })
            }
            None => json!({ "error": "no window" }),
        },
        // Answers the open question.
        Some("answer") => in_window(controller, text("window"), cx, |shell, window, cx| {
            shell.update(cx, |shell, cx| shell.answer_question(flag("yes"), window, cx));
            json!({ "ok": true })
        }),
        Some("workspace") => {
            let window_id = text("window")
                .or_else(|| {
                    controller
                        .read(cx)
                        .state()
                        .windows
                        .first()
                        .map(|window| window.id.clone())
                })
                .unwrap_or_default();
            let id = text("id").unwrap_or_default();
            let name = text("name").unwrap_or_default();
            controller.update(cx, |controller, cx| match text("action").as_deref() {
                Some("save") => controller.save_workspace(&window_id, &name, cx),
                Some("update") => controller.update_workspace(&window_id, cx),
                Some("open") => controller.open_workspace(&id, cx),
                Some("rename") => controller.rename_workspace(&id, &name, cx),
                Some("delete") => controller.delete_workspace(&window_id, &id, cx),
                _ => {}
            });
            json!({ "ok": true })
        }
        Some("quit") => {
            controller.update(cx, |controller, cx| controller.shutdown(cx));
            json!({ "ok": true })
        }
        other => json!({ "error": format!("unknown command: {other:?}") }),
    }
}

/// A menu as tests read it: every entry's label, whether it can be picked, and its submenu.
fn tray_json(entry: &TrayEntry) -> Value {
    match entry {
        TrayEntry::Separator => Value::Null,
        TrayEntry::Item { label, checked, .. } => json!({ "label": label, "checked": checked }),
        TrayEntry::Submenu { label, children } => {
            json!({ "label": label, "children": children.iter().map(tray_json).collect::<Vec<_>>() })
        }
    }
}

/// The command of the tray menu's entry with that label, in submenus too.
fn tray_command(entries: &[TrayEntry], label: &str) -> Option<u32> {
    entries.iter().find_map(|entry| match entry {
        TrayEntry::Item {
            id,
            label: found,
            enabled,
            ..
        } if found == label && *enabled => Some(*id),
        TrayEntry::Submenu { children, .. } => tray_command(children, label),
        _ => None,
    })
}

fn list_json(list: &SessionList) -> Value {
    json!({
        "workspaces": list.workspaces.iter().map(|item| json!({
            "id": item.id,
            "name": item.name,
            "sessions": item.sessions,
            "open": item.open,
        })).collect::<Vec<_>>(),
        "app": list.app.iter().map(|item| json!({
            "id": item.id,
            "label": item.label,
            "cwd": item.cwd,
            "status": status_name(item.status),
            "window": item.window_id,
            "windowNumber": item.window_number,
            "lastActiveAt": item.last_active_at,
            "elsewhere": item.elsewhere,
        })).collect::<Vec<_>>(),
        "running": list.running.iter().map(|item| json!({
            "pid": item.pid,
            "claudeSessionId": item.claude_session_id,
            "cwd": item.cwd,
            "name": item.name,
        })).collect::<Vec<_>>(),
        "found": list.found.iter().map(|item| json!({
            "claudeSessionId": item.claude_session_id,
            "cwd": item.cwd,
            "title": item.title,
            "modifiedAt": item.modified_at,
        })).collect::<Vec<_>>(),
        "hasMore": list.has_more,
    })
}

fn menu_json(entries: &[MenuEntry]) -> Value {
    entries
        .iter()
        .map(|entry| {
            json!({
                "label": entry.label,
                "enabled": entry.enabled,
                "checked": entry.checked,
                "children": menu_json(&entry.children),
            })
        })
        .collect()
}

/// Runs something in a window: the one with that id, or the first one.
fn in_window(
    controller: &Entity<Controller>,
    window_id: Option<String>,
    cx: &mut App,
    run: impl FnOnce(&Entity<Shell>, &mut Window, &mut App) -> Value,
) -> Value {
    let Some(shell) = shell_of(controller, window_id, cx) else {
        return json!({ "error": "no window" });
    };
    let handle = shell.read(cx).handle();
    handle
        .update(cx, |_, window, cx| run(&shell, window, cx))
        .unwrap_or_else(|error| json!({ "error": error.to_string() }))
}

/// The window with that id, or the first one.
fn shell_of(controller: &Entity<Controller>, window_id: Option<String>, cx: &App) -> Option<Entity<Shell>> {
    let id = match window_id {
        Some(id) => id,
        None => controller.read(cx).state().windows.first()?.id.clone(),
    };
    window::shell(&id, cx)
}
