//! A process on a pseudo console, with its terminal state.
//!
//! The terminal state (`Term`) lives once per process; any number of views in any window can
//! draw it. That replaces the Electron version's PTY host with its headless mirror and the
//! serialized snapshots sent to each view (`src/ptyhost/`).

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty::{self, Options, Shell};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::colors;

pub const SCROLLBACK: usize = 5000;
/// The size a process starts with, before any view has measured itself (as in the Electron
/// version).
pub const INITIAL_COLS: u16 = 120;
pub const INITIAL_ROWS: u16 = 32;

/// Prepares this process's environment, which every child inherits: the terminal type, and none
/// of the markers of a parent Claude Code session (see `reframe_core::claude::env`). Call once,
/// before any thread is started.
pub fn prepare_environment() {
    let markers: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| reframe_core::claude::env::is_parent_session_marker(name))
        .collect();
    for name in markers {
        unsafe { std::env::remove_var(name) };
    }
    unsafe {
        std::env::set_var("TERM", "xterm-256color");
        std::env::set_var("COLORTERM", "truecolor");
    }
}

/// What to run.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    /// The arguments are one ready-made command line (for cmd.exe) and must not be quoted again.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub raw_args: bool,
    pub cwd: PathBuf,
    /// Variables on top of this process's environment.
    pub env: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// Receives the terminal's events on the pty thread. Replies the program is waiting for go
/// straight back to the pty; everything else is passed to the UI.
#[derive(Clone)]
pub struct Listener {
    ui: UnboundedSender<Event>,
    pty: Arc<OnceLock<EventLoopSender>>,
    size: Arc<Mutex<WindowSize>>,
}

impl Listener {
    fn reply(&self, text: String) {
        if let Some(pty) = self.pty.get() {
            let _ = pty.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.reply(text),
            Event::ColorRequest(index, format) => self.reply(format(colors::indexed(index))),
            Event::TextAreaSizeRequest(format) => {
                let size = self.size.lock().map(|size| *size);
                if let Ok(size) = size {
                    self.reply(format(size));
                }
            }
            other => {
                let _ = self.ui.unbounded_send(other);
            }
        }
    }
}

/// A handle to a running process and its terminal state. Cheap to clone; the process ends when
/// [`Pty::shutdown`] is called (or when it exits by itself), not when a handle is dropped.
#[derive(Clone)]
pub struct Pty {
    pub term: Arc<FairMutex<Term<Listener>>>,
    pub pid: Option<u32>,
    sender: EventLoopSender,
    size: Arc<Mutex<WindowSize>>,
}

fn window_size(size: TermSize, cell_width: u16, cell_height: u16) -> WindowSize {
    WindowSize {
        num_lines: size.rows,
        num_cols: size.cols,
        cell_width,
        cell_height,
    }
}

impl Pty {
    /// Starts the process. The receiver yields the events the UI has to act on (wakeups, title,
    /// bell, exit).
    pub fn spawn(launch: &Launch) -> std::io::Result<(Pty, UnboundedReceiver<Event>)> {
        let size = TermSize {
            cols: INITIAL_COLS,
            rows: INITIAL_ROWS,
        };
        let initial = window_size(size, 8, 17);
        let (ui, events) = unbounded();
        let listener = Listener {
            ui,
            pty: Arc::new(OnceLock::new()),
            size: Arc::new(Mutex::new(initial)),
        };

        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));

        // Windows starts a command line: a program with a space in it is quoted. Elsewhere the
        // program is started as it is named.
        let program = if cfg!(windows) && launch.program.contains(' ') && !launch.program.starts_with('"') {
            format!("\"{}\"", launch.program)
        } else {
            launch.program.clone()
        };
        let options = Options {
            shell: Some(Shell::new(program, launch.args.clone())),
            working_directory: Some(launch.cwd.clone()),
            drain_on_exit: false,
            env: launch.env.clone(),
            #[cfg(windows)]
            escape_args: !launch.raw_args,
        };
        let pty = tty::new(&options, initial, 0)?;
        #[cfg(windows)]
        let pid = pty.child_watcher().pid().map(|pid| pid.get());
        #[cfg(not(windows))]
        let pid = Some(pty.child().id());

        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, false, false)?;
        let sender = event_loop.channel();
        let _ = listener.pty.set(sender.clone());
        event_loop.spawn();

        Ok((
            Pty {
                term,
                pid,
                sender,
                size: listener.size,
            },
            events,
        ))
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    /// Resizes the terminal state and the pty together.
    pub fn resize(&self, size: TermSize, cell_width: u16, cell_height: u16) {
        let window = window_size(size, cell_width, cell_height);
        if let Ok(mut current) = self.size.lock() {
            *current = window;
        }
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(window));
    }

    /// Closes the pseudo console, which ends what still runs on it.
    pub fn shutdown(&self) {
        let _ = self.sender.send(Msg::Shutdown);
    }

    /// The visible screen as text, one line per row, trailing blanks removed.
    pub fn screen_text(&self) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let mut text = String::new();
        for row in 0..grid.screen_lines() {
            let mut line = String::new();
            for col in 0..grid.columns() {
                let cell = &grid[Line(row as i32)][Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                line.push(cell.c);
                if let Some(extra) = cell.zerowidth() {
                    line.extend(extra);
                }
            }
            text.push_str(line.trim_end());
            text.push('\n');
        }
        text
    }
}
