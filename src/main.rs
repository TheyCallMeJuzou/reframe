mod colors;
mod session;

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::*;

use session::{Launch, Session};

struct Args {
    claude: Option<String>,
    tiles: Vec<PathBuf>,
    dump: bool,
}

fn parse_args() -> Args {
    let mut args = Args { claude: None, tiles: Vec::new(), dump: false };
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--dump" => args.dump = true,
            "--claude" => args.claude = input.next(),
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

fn launch_for(claude: &str, folder: &PathBuf) -> Launch {
    Launch {
        program: claude.to_owned(),
        args: vec!["--session-id".into(), uuid::Uuid::new_v4().to_string()],
        cwd: folder.clone(),
    }
}

/// Starts one session without any window, waits, and prints what is on its screen.
fn dump(launch: &Launch) {
    let (session, _events) = Session::spawn(launch).expect("failed to start the session");
    std::thread::sleep(Duration::from_secs(8));
    println!("{}", session.screen_text());

    let pid = session.pid.expect("no pid");
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude")))
        .expect("no home folder");
    let registry = config.join("sessions").join(format!("{pid}.json"));
    println!("--- pid {pid}, registry entry {}: {}", registry.display(), registry.is_file());

    // Two Ctrl+C end claude cleanly.
    session.write(&b"\x03"[..]);
    std::thread::sleep(Duration::from_millis(400));
    session.write(&b"\x03"[..]);
    std::thread::sleep(Duration::from_secs(2));
    println!("--- registry entry after exit: {}", registry.is_file());
}

struct Shell;

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let pane = |label: &'static str| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(0x17181c))
                .text_color(rgb(0xd7dae0))
                .child(label)
        };
        div().size_full().bg(rgb(0x17181c)).child(
            h_resizable("tiles")
                .child(resizable_panel().child(pane("tile 1")))
                .child(resizable_panel().child(pane("tile 2"))),
        )
    }
}

fn main() {
    session::prepare_environment();
    let args = parse_args();
    let claude = args.claude.clone().or_else(find_claude).unwrap_or_else(|| {
        eprintln!("claude.exe is not on the PATH; pass --claude <path>");
        std::process::exit(1);
    });

    if args.dump {
        let folder = args.tiles.first().cloned().unwrap_or_else(|| std::env::current_dir().unwrap());
        dump(&launch_for(&claude, &folder));
        return;
    }

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Shell))
            .expect("failed to open window");
    });
}
