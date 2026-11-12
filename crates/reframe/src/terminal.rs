//! One start of a session's process, as the UI sees it: its pty, its title, and whether it has
//! exited. Views observe it and redraw when it changes.

use alacritty_terminal::event::Event as TermEvent;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::*;

use crate::pty::Pty;

pub enum TerminalEvent {
    /// The process exited, with its exit code.
    Exited(i32),
}

pub struct Terminal {
    pty: Pty,
    pub title: Option<String>,
    pub exit_code: Option<i32>,
    _events: Task<()>,
}

impl EventEmitter<TerminalEvent> for Terminal {}

impl Terminal {
    /// Takes over a process that was just started, with the events of its pty.
    pub fn new(pty: Pty, mut events: UnboundedReceiver<TermEvent>, cx: &mut Context<Self>) -> Self {
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let mut batch = vec![event];
                while let Ok(event) = events.try_recv() {
                    batch.push(event);
                }
                let alive = this.update(cx, |this, cx| {
                    for event in batch {
                        this.on_event(event, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Self {
            pty,
            title: None,
            exit_code: None,
            _events: task,
        }
    }

    fn on_event(&mut self, event: TermEvent, cx: &mut Context<Self>) {
        match event {
            TermEvent::Title(title) => self.title = Some(title),
            TermEvent::ResetTitle => self.title = None,
            TermEvent::ChildExit(status) => {
                // Reported once, also when the pty's own end follows.
                if self.exit_code.is_none() {
                    let code = status.code().unwrap_or(-1);
                    self.exit_code = Some(code);
                    cx.emit(TerminalEvent::Exited(code));
                }
            }
            TermEvent::ClipboardStore(_, text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            _ => {}
        }
        cx.notify();
    }

    pub fn pty(&self) -> &Pty {
        &self.pty
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.pty.shutdown();
    }
}
