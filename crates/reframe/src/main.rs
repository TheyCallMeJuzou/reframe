// Release builds are a window application: no console window opens next to it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod colors;
mod glyphs;
mod input;
mod session;
mod terminal_view;

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::*;

use session::{Launch, Session};
use terminal_view::{TerminalView, TileEvent};

struct Args {
    claude: Option<String>,
    /// claude's path inside WSL and the login shell's PATH there, for tiles under \\wsl.localhost.
    wsl_claude: Option<String>,
    wsl_path: Option<String>,
    tiles: Vec<PathBuf>,
    dump: bool,
    selftest: bool,
    /// For measurements: end the sessions and quit after this many seconds.
    hold: Option<u64>,
    /// Times reading the clipboard and saving its image, prints the result and quits.
    clip_test: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        claude: None,
        wsl_claude: None,
        wsl_path: None,
        tiles: Vec::new(),
        dump: false,
        selftest: false,
        hold: None,
        clip_test: false,
    };
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--dump" => args.dump = true,
            "--selftest" => args.selftest = true,
            "--clip-test" => args.clip_test = true,
            "--hold" => args.hold = input.next().and_then(|seconds| seconds.parse().ok()),
            "--claude" => args.claude = input.next(),
            "--wsl-claude" => args.wsl_claude = input.next(),
            "--wsl-path" => args.wsl_path = input.next(),
            "--tile" => args.tiles.extend(input.next().map(PathBuf::from)),
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    args
}

/// The first `claude.exe` on the PATH. The npm `.cmd` shim is skipped on purpose: the pty's
/// process has to be claude itself, because Claude Code's session registry is keyed by its pid.
fn find_claude() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("claude.exe"))
        .find(|file| file.is_file())
        .map(|file| file.to_string_lossy().into_owned())
}

/// `\\wsl.localhost\<distro>\home\me` or `\\wsl$\<distro>\...` as (distro, `/home/me`).
fn wsl_folder(folder: &Path) -> Option<(String, String)> {
    let text = folder.to_string_lossy().replace('/', "\\");
    let rest = text
        .strip_prefix("\\\\wsl.localhost\\")
        .or_else(|| text.strip_prefix("\\\\wsl$\\"))?;
    let (distro, path) = rest.split_once('\\').unwrap_or((rest, ""));
    Some((distro.to_owned(), format!("/{}", path.replace('\\', "/"))))
}

fn launch_for(args: &Args, claude: &str, folder: &Path) -> Launch {
    let session = vec!["--session-id".to_owned(), uuid::Uuid::new_v4().to_string()];
    let Some((distro, linux_path)) = wsl_folder(folder) else {
        return Launch {
            program: claude.to_owned(),
            args: session,
            cwd: folder.to_owned(),
            wsl: false,
        };
    };
    // No shell in between, so nothing the startup files print gets into the terminal.
    let wsl_claude = args
        .wsl_claude
        .clone()
        .expect("a WSL tile needs --wsl-claude <path of claude in the distro>");
    let wsl_path = args
        .wsl_path
        .clone()
        .expect("a WSL tile needs --wsl-path <the login shell's PATH>");
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let mut wsl_args: Vec<String> = [
        "-d",
        &distro,
        "--cd",
        &linux_path,
        "--exec",
        "env",
        &format!("PATH={wsl_path}"),
        &wsl_claude,
    ]
    .map(String::from)
    .to_vec();
    wsl_args.extend(session);
    Launch {
        program: format!("{system_root}\\System32\\wsl.exe"),
        args: wsl_args,
        cwd: std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| folder.to_owned()),
        wsl: true,
    }
}

fn registry_entry(pid: u32) -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude")))
        .expect("no home folder")
        .join("sessions")
        .join(format!("{pid}.json"))
}

/// Starts one session without any window, waits, and prints what is on its screen.
fn dump(launch: &Launch) {
    let (session, _events) = Session::spawn(launch).expect("failed to start the session");
    std::thread::sleep(Duration::from_secs(8));
    println!("{}", session.screen_text());
    print!("{}", session.color_summary());
    // RFN_PASTE=<text>: paste it the way the tile does and show what the prompt makes of it.
    if let Ok(text) = std::env::var("RFN_PASTE") {
        session.write(input::paste_bytes(&text, true));
        std::thread::sleep(Duration::from_secs(4));
        println!("--- after pasting {text:?}");
        for line in session
            .screen_text()
            .lines()
            .filter(|line| line.contains('❯') || line.contains("Image"))
        {
            println!("{line}");
        }
    }

    let pid = session.pid.expect("no pid");
    let registry = registry_entry(pid);
    println!(
        "--- pid {pid}, registry entry {}: {}",
        registry.display(),
        registry.is_file()
    );

    // Two Ctrl+C end claude cleanly.
    session.write(&b"\x03"[..]);
    std::thread::sleep(Duration::from_millis(400));
    session.write(&b"\x03"[..]);
    std::thread::sleep(Duration::from_secs(2));
    println!("--- registry entry after exit: {}", registry.is_file());
}

/// A window: its tiles side by side.
struct Shell {
    tiles: Vec<Entity<TerminalView>>,
    subscriptions: Vec<Subscription>,
    /// A tile that gets the keyboard focus with the next frame.
    focus_next: Option<Entity<TerminalView>>,
}

impl Shell {
    fn new(tiles: Vec<Entity<TerminalView>>, cx: &mut Context<Self>) -> Self {
        let subscriptions = tiles
            .iter()
            .map(|tile| cx.subscribe(tile, Self::on_tile_event))
            .collect();
        Self {
            tiles,
            subscriptions,
            focus_next: None,
        }
    }

    fn on_tile_event(&mut self, tile: Entity<TerminalView>, event: &TileEvent, cx: &mut Context<Self>) {
        match event {
            TileEvent::TearOut => self.tear_out(&tile, cx),
        }
    }

    /// Moves a tile into a window of its own. The session and its terminal state are untouched;
    /// only the entity that draws them changes windows.
    fn tear_out(&mut self, tile: &Entity<TerminalView>, cx: &mut Context<Self>) {
        if self.tiles.len() < 2 {
            return;
        }
        let Some(index) = self.tiles.iter().position(|other| other == tile) else {
            return;
        };
        self.tiles.remove(index);
        drop(self.subscriptions.remove(index));
        // The window keeps a focused tile: the one that moved up into the gap.
        self.focus_next = self.tiles.get(index.min(self.tiles.len() - 1)).cloned();
        cx.notify();
        let tile = tile.clone();
        cx.defer(move |cx| {
            open_shell(vec![tile], cx);
        });
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(tile) = self.focus_next.take() {
            let focus = tile.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        let mut group = h_resizable("tiles");
        for tile in &self.tiles {
            group = group.child(resizable_panel().child(tile.clone()));
        }
        div().size_full().bg(rgb(0x17181c)).child(group)
    }
}

fn open_shell(tiles: Vec<Entity<TerminalView>>, cx: &mut App) -> Entity<Shell> {
    let first = tiles.first().cloned();
    let (_, shell) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
        window.set_window_title("reframe native");
        if let Some(tile) = &first {
            let focus = tile.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        cx.new(|cx| Shell::new(tiles, cx))
    })
    .expect("failed to open window");
    shell
}

/// Runs without anyone at the keyboard: lets the sessions draw, types and selects through the
/// window's real input path, tears the first tile out into its own window, and ends the sessions.
/// It prints what it found; nothing is sent to Claude as a prompt.
async fn selftest(shell: Entity<Shell>, tiles: Vec<Entity<TerminalView>>, cx: &mut AsyncApp) {
    let report = |label: &str, cx: &mut AsyncApp| {
        println!("=== {label}");
        for tile in &tiles {
            let (pid, text) = tile.read_with(cx, |tile, _| {
                (tile.session.pid.unwrap_or(0), tile.session.screen_text())
            });
            let banner = text
                .lines()
                .find(|line| line.contains("Claude Code"))
                .unwrap_or("(no banner)")
                .trim();
            let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
            println!(
                "pid {pid}: registry entry {}, {lines} non-empty lines, banner: {}",
                registry_entry(pid).is_file(),
                banner.chars().take(40).collect::<String>()
            );
        }
    };
    let pause = |millis: u64, cx: &mut AsyncApp| cx.background_executor().timer(Duration::from_millis(millis));
    let first = tiles[0].clone();
    let window = cx.update(|cx| cx.windows()[0]);
    let prompt_line = |cx: &mut AsyncApp| {
        let text = first.read_with(cx, |tile, _| tile.session.screen_text());
        text.lines()
            .find(|line| line.starts_with('❯'))
            .unwrap_or("(no prompt line)")
            .to_owned()
    };

    pause(10_000, cx).await;
    report("after start", cx);
    print!("{}", first.read_with(cx, |tile, _| tile.session.color_summary()));
    println!("mode: {:?}", first.read_with(cx, |tile, _| tile.mode()));

    // Typing: letters go through the text-input path, Backspace through the key path.
    for key in ["h", "e", "l", "l", "o"] {
        let _ = window.update(cx, |_, window, cx| {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx)
        });
    }
    pause(800, cx).await;
    println!("typed 'hello'      -> {}", prompt_line(cx));
    for _ in 0..2 {
        let _ = window.update(cx, |_, window, cx| {
            window.dispatch_keystroke(Keystroke::parse("backspace").unwrap(), cx)
        });
    }
    pause(800, cx).await;
    println!("two backspaces     -> {}", prompt_line(cx));

    // Selecting with the mouse: drag across the first eight characters of the prompt line.
    let row = first.read_with(cx, |tile, _| {
        tile.session
            .screen_text()
            .lines()
            .position(|line| line.starts_with('❯'))
            .unwrap_or(0)
    });
    let shift = first.read_with(cx, |tile, _| {
        tile.mode().intersects(alacritty_terminal::term::TermMode::MOUSE_MODE)
    });
    let modifiers = Modifiers {
        shift,
        ..Default::default()
    };
    let at = |col: usize, cx: &mut AsyncApp| first.read_with(cx, |tile, _| tile.cell_center(col, row).unwrap());
    let (from, to) = (at(0, cx), at(7, cx));
    let _ = window.update(cx, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: from,
                modifiers,
                ..Default::default()
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers,
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: to,
                pressed_button: Some(MouseButton::Left),
                modifiers,
                ..Default::default()
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers,
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
    });
    println!(
        "mouse drag selects -> {:?}",
        first.read_with(cx, |tile, _| tile.selection_text())
    );
    let _ = window.update(cx, |_, window, cx| {
        window.dispatch_keystroke(Keystroke::parse("backspace").unwrap(), cx)
    });

    // Tear-out.
    let size_before = first.read_with(cx, |tile, _| tile.grid_size());
    shell.update(cx, |shell, cx| shell.tear_out(&first, cx));
    pause(4_000, cx).await;
    report("after tear-out", cx);
    println!(
        "windows: {}, torn-out tile {:?} -> {:?} cells, prompt: {}",
        cx.update(|cx| cx.windows().len()),
        size_before,
        first.read_with(cx, |tile, _| tile.grid_size()),
        prompt_line(cx)
    );
    pause(3_000, cx).await;

    // Two Ctrl+C end claude; sent as keys to whichever tile has the focus in each window.
    for _ in 0..2 {
        for window in cx.update(|cx| cx.windows()) {
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_keystroke(Keystroke::parse("ctrl-c").unwrap(), cx)
            });
        }
        pause(400, cx).await;
    }
    pause(3_000, cx).await;
    report("after two Ctrl+C in each window", cx);
    for tile in &tiles {
        println!("exited: {}", tile.read_with(cx, |tile, _| tile.exited));
    }
    cx.update(|cx| cx.quit());
}

/// Leaves the app idle for a while, then ends the sessions the way a user would and quits.
async fn hold(seconds: u64, tiles: Vec<Entity<TerminalView>>, cx: &mut AsyncApp) {
    cx.background_executor().timer(Duration::from_secs(seconds)).await;
    for _ in 0..2 {
        for tile in &tiles {
            tile.read_with(cx, |tile, _| tile.session.write(&b""[..]));
        }
        cx.background_executor().timer(Duration::from_millis(400)).await;
    }
    cx.background_executor().timer(Duration::from_secs(3)).await;
    cx.update(|cx| cx.quit());
}

fn main() {
    session::prepare_environment();
    let args = parse_args();
    let claude = args.claude.clone().or_else(find_claude).unwrap_or_else(|| {
        eprintln!("claude.exe is not on the PATH; pass --claude <path>");
        std::process::exit(1);
    });
    let folders = if args.tiles.is_empty() {
        vec![std::env::current_dir().unwrap()]
    } else {
        args.tiles.clone()
    };
    let launches: Vec<Launch> = folders
        .iter()
        .map(|folder| launch_for(&args, &claude, folder))
        .collect();

    if args.dump {
        dump(&launches[0]);
        return;
    }

    if args.clip_test {
        gpui_kit::application().run(|cx| {
            let started = std::time::Instant::now();
            let item = cx.read_from_clipboard();
            println!("read clipboard: {} ms", started.elapsed().as_millis());
            for entry in item.iter().flat_map(|item| item.entries()) {
                match entry {
                    ClipboardEntry::String(_) => println!("entry: text"),
                    ClipboardEntry::ExternalPaths(_) => println!("entry: files"),
                    ClipboardEntry::Image(image) => {
                        println!("entry: image {:?}, {} bytes", image.format, image.bytes.len());
                        let started = std::time::Instant::now();
                        let file = terminal_view::save_clipboard_image(image);
                        println!("saved in {} ms: {file:?}", started.elapsed().as_millis());
                        if let Some(file) = file {
                            let _ = std::fs::remove_file(file);
                        }
                    }
                }
            }
            cx.quit();
        });
        return;
    }

    let run_selftest = args.selftest;
    let hold_seconds = args.hold;
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let tiles: Vec<Entity<TerminalView>> = launches
            .iter()
            .map(|launch| cx.new(|cx| TerminalView::new(launch, cx)))
            .collect();
        let shell = open_shell(tiles.clone(), cx);
        if run_selftest {
            cx.spawn(async move |cx| selftest(shell, tiles, cx).await).detach();
        } else if let Some(seconds) = hold_seconds {
            cx.spawn(async move |cx| hold(seconds, tiles, cx).await).detach();
        }
    });
}
