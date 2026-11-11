//! One claude process on a ConPTY, with its terminal state.
//!
//! The terminal state (`Term`) lives once per session; any number of views in any window can draw
//! it. That replaces the Electron version's headless mirror and its serialized snapshots.

use std::borrow::Cow;
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
pub const INITIAL_COLS: u16 = 120;
pub const INITIAL_ROWS: u16 = 32;

/// Markers a parent Claude Code session leaves in the environment. A claude that inherits them
/// runs as a "child session": no transcript, no registry entry. Same list as the Electron
/// version's `src/main/claude/env.ts`.
const PARENT_SESSION_VARS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_ENTRYPOINT",
];

/// Prepares this process's environment, which every session inherits. Call once, before any
/// thread is started.
pub fn prepare_environment() {
    let names: Vec<String> = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| PARENT_SESSION_VARS.iter().any(|v| v.eq_ignore_ascii_case(name)))
        .collect();
    for name in names {
        unsafe { std::env::remove_var(name) };
    }
    unsafe {
        std::env::set_var("TERM", "xterm-256color");
        std::env::set_var("COLORTERM", "truecolor");
    }
}

/// What to run for a session.
#[derive(Clone, Debug)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The session runs inside WSL: Windows paths handed to it are written as `/mnt/<drive>/...`.
    pub wsl: bool,
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
                let size = *self.size.lock().unwrap();
                self.reply(format(size));
            }
            other => {
                let _ = self.ui.unbounded_send(other);
            }
        }
    }
}

pub struct Session {
    pub term: Arc<FairMutex<Term<Listener>>>,
    pub pid: Option<u32>,
    pty: EventLoopSender,
    size: Arc<Mutex<WindowSize>>,
}

fn window_size(size: TermSize, cell_width: u16, cell_height: u16) -> WindowSize {
    WindowSize { num_lines: size.rows, num_cols: size.cols, cell_width, cell_height }
}

impl Session {
    /// Starts the process. The receiver yields the events the UI has to act on (wakeups, title,
    /// bell, exit).
    pub fn spawn(launch: &Launch) -> std::io::Result<(Session, UnboundedReceiver<Event>)> {
        let size = TermSize { cols: INITIAL_COLS, rows: INITIAL_ROWS };
        let initial = window_size(size, 8, 17);
        let (ui, events) = unbounded();
        let listener = Listener {
            ui,
            pty: Arc::new(OnceLock::new()),
            size: Arc::new(Mutex::new(initial)),
        };

        let config = Config { scrolling_history: SCROLLBACK, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));

        let program = if launch.program.contains(' ') && !launch.program.starts_with('"') {
            format!("\"{}\"", launch.program)
        } else {
            launch.program.clone()
        };
        let options = Options {
            shell: Some(Shell::new(program, launch.args.clone())),
            working_directory: Some(launch.cwd.clone()),
            drain_on_exit: false,
            env: Default::default(),
            escape_args: true,
        };
        let pty = tty::new(&options, initial, 0)?;
        let pid = pty.child_watcher().pid().map(|pid| pid.get());

        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, false, false)?;
        let sender = event_loop.channel();
        let _ = listener.pty.set(sender.clone());
        event_loop.spawn();

        Ok((Session { term, pid, pty: sender, size: listener.size }, events))
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    /// Resizes the terminal state and the pty together.
    pub fn resize(&self, size: TermSize, cell_width: u16, cell_height: u16) {
        let window = window_size(size, cell_width, cell_height);
        *self.size.lock().unwrap() = window;
        self.term.lock().resize(size);
        let _ = self.pty.send(Msg::Resize(window));
    }

    pub fn shutdown(&self) {
        let _ = self.pty.send(Msg::Shutdown);
    }

    /// How many visible characters use each colour pair; for checking what the program sent.
    pub fn color_summary(&self) -> String {
        let term = self.term.lock();
        let mut seen = std::collections::BTreeMap::new();
        for cell in term.grid().display_iter() {
            if cell.c != ' ' {
                *seen.entry(format!("{:?} on {:?}", cell.fg, cell.bg)).or_insert(0usize) += 1;
            }
        }
        seen.into_iter().map(|(colors, count)| format!("{count:5}  {colors}
")).collect()
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
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
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

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}
