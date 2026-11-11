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
}

fn parse_args() -> Args {
    let mut args =
        Args { claude: None, wsl_claude: None, wsl_path: None, tiles: Vec::new(), dump: false, selftest: false };
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--dump" => args.dump = true,
            "--selftest" => args.selftest = true,
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
    let rest = text.strip_prefix("\\\\wsl.localhost\\").or_else(|| text.strip_prefix("\\\\wsl$\\"))?;
    let (distro, path) = rest.split_once('\\').unwrap_or((rest, ""));
    Some((distro.to_owned(), format!("/{}", path.replace('\\', "/"))))
}

fn launch_for(args: &Args, claude: &str, folder: &Path) -> Launch {
    let session = vec!["--session-id".to_owned(), uuid::Uuid::new_v4().to_string()];
    let Some((distro, linux_path)) = wsl_folder(folder) else {
        return Launch { program: claude.to_owned(), args: session, cwd: folder.to_owned() };
    };
    // No shell in between, so nothing the startup files print gets into the terminal.
    let wsl_claude = args.wsl_claude.clone().expect("a WSL tile needs --wsl-claude <path of claude in the distro>");
    let wsl_path = args.wsl_path.clone().expect("a WSL tile needs --wsl-path <the login shell's PATH>");
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let mut wsl_args: Vec<String> =
        ["-d", &distro, "--cd", &linux_path, "--exec", "env", &format!("PATH={wsl_path}"), &wsl_claude]
            .map(String::from)
            .to_vec();
    wsl_args.extend(session);
    Launch {
        program: format!("{system_root}\\System32\\wsl.exe"),
        args: wsl_args,
        cwd: std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| folder.to_owned()),
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

    let pid = session.pid.expect("no pid");
    let registry = registry_entry(pid);
    println!("--- pid {pid}, registry entry {}: {}", registry.display(), registry.is_file());

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
}

impl Shell {
    fn new(tiles: Vec<Entity<TerminalView>>, cx: &mut Context<Self>) -> Self {
        let subscriptions = tiles.iter().map(|tile| cx.subscribe(tile, Self::on_tile_event)).collect();
        Self { tiles, subscriptions }
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
        let Some(index) = self.tiles.iter().position(|other| other == tile) else { return };
        self.tiles.remove(index);
        drop(self.subscriptions.remove(index));
        cx.notify();
        let tile = tile.clone();
        cx.defer(move |cx| {
            open_shell(vec![tile], cx);
        });
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
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

/// Runs without anyone at the keyboard: waits for the sessions to draw, tears the first tile out
/// into its own window, and reports whether each session survived with its screen.
async fn selftest(shell: Entity<Shell>, tiles: Vec<Entity<TerminalView>>, cx: &mut AsyncApp) {
    let report = |label: &str, cx: &mut AsyncApp| {
        println!("=== {label}");
        for tile in &tiles {
            let (pid, text) = tile.read_with(cx, |tile, _| (tile.session.pid.unwrap_or(0), tile.session.screen_text()));
            let banner = text.lines().find(|line| line.contains("Claude Code")).unwrap_or("(no banner)").trim();
            let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
            println!(
                "pid {pid}: registry entry {}, {lines} non-empty lines, banner: {}",
                registry_entry(pid).is_file(),
                banner.chars().take(60).collect::<String>()
            );
        }
    };

    cx.background_executor().timer(Duration::from_secs(10)).await;
    report("before tear-out", cx);
    let first = tiles[0].clone();
    shell.update(cx, |shell, cx| shell.tear_out(&first, cx));
    cx.background_executor().timer(Duration::from_secs(4)).await;
    report("after tear-out", cx);
    let windows = cx.update(|cx| cx.windows().len());
    println!("windows: {windows}");
    print!("{}", first.read_with(cx, |tile, _| tile.session.color_summary()));
    println!("--- screen of the torn-out tile");
    println!("{}", first.read_with(cx, |tile, _| tile.session.screen_text()));

    for tile in &tiles {
        tile.read_with(cx, |tile, _| tile.session.write(&b"\x03"[..]));
    }
    cx.background_executor().timer(Duration::from_millis(400)).await;
    for tile in &tiles {
        tile.read_with(cx, |tile, _| tile.session.write(&b"\x03"[..]));
    }
    cx.background_executor().timer(Duration::from_secs(3)).await;
    report("after two Ctrl+C", cx);
    cx.update(|cx| cx.quit());
}

fn main() {
    session::prepare_environment();
    let args = parse_args();
    let claude = args.claude.clone().or_else(find_claude).unwrap_or_else(|| {
        eprintln!("claude.exe is not on the PATH; pass --claude <path>");
        std::process::exit(1);
    });
    let folders =
        if args.tiles.is_empty() { vec![std::env::current_dir().unwrap()] } else { args.tiles.clone() };
    let launches: Vec<Launch> = folders.iter().map(|folder| launch_for(&args, &claude, folder)).collect();

    if args.dump {
        dump(&launches[0]);
        return;
    }

    let run_selftest = args.selftest;
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let tiles: Vec<Entity<TerminalView>> =
            launches.iter().map(|launch| cx.new(|cx| TerminalView::new(launch, cx))).collect();
        let shell = open_shell(tiles.clone(), cx);
        if run_selftest {
            cx.spawn(async move |cx| selftest(shell, tiles, cx).await).detach();
        }
    });
}
