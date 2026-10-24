// One xterm instance per session, kept alive while the session is in this window. Tiles only lend it a
// slot in the DOM: moving a tile re-parents the same terminal element, so nothing is redrawn or lost.
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebglAddon } from '@xterm/addon-webgl';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { TERMINAL_PORT_MESSAGE, type TerminalHostMessage, type TerminalViewMessage } from '@shared/ipc';
import { formatPaths, imagePasteKey } from './paste';

interface Entry {
  sessionId: string;
  term: Terminal;
  fit: FitAddon;
  element: HTMLDivElement;
  port: MessagePort | null;
  observer: ResizeObserver;
  opened: boolean;
  lastSize: { cols: number; rows: number } | null;
  frame: number;
}

const FONT = '"Cascadia Mono", "Cascadia Code", Consolas, "SF Mono", Menlo, "DejaVu Sans Mono", monospace';

const THEME = {
  background: '#17181c',
  foreground: '#d7dae0',
  cursor: '#d7dae0',
  selectionBackground: '#3a3f4b',
};

export class TerminalRegistry {
  private entries = new Map<string, Entry>();

  constructor() {
    window.addEventListener('message', (event: MessageEvent) => {
      const data = event.data as { type?: string; sessionId?: string } | null;
      if (data?.type !== TERMINAL_PORT_MESSAGE || !data.sessionId || !event.ports[0]) return;
      this.connect(data.sessionId, event.ports[0]);
    });
  }

  /** Shows the session's terminal inside `slot`, creating and attaching it on first use. */
  mount(sessionId: string, slot: HTMLElement): void {
    const entry = this.entries.get(sessionId) ?? this.create(sessionId);
    slot.appendChild(entry.element);
    entry.observer.observe(entry.element);
    if (!entry.opened) {
      entry.term.open(entry.element);
      entry.opened = true;
      try {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => webgl.dispose());
        entry.term.loadAddon(webgl);
      } catch {
        // the DOM renderer is used instead
      }
    }
    this.scheduleFit(entry);
  }

  /** Takes the terminal out of `slot` (if it is still there) without ending anything. */
  unmount(sessionId: string, slot: HTMLElement): void {
    const entry = this.entries.get(sessionId);
    if (!entry || entry.element.parentElement !== slot) return;
    entry.observer.unobserve(entry.element);
    slot.removeChild(entry.element);
  }

  /** Forgets sessions that left this window. */
  retain(sessionIds: Set<string>): void {
    for (const [id, entry] of this.entries) {
      if (sessionIds.has(id)) continue;
      entry.port?.close();
      entry.observer.disconnect();
      entry.term.dispose();
      entry.element.remove();
      this.entries.delete(id);
    }
  }

  /** Clears the screen and asks for a fresh connection, e.g. after the session was restarted. */
  reattach(sessionId: string): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;
    entry.port?.close();
    entry.port = null;
    entry.term.reset();
    window.reframe.attachTerminal(sessionId);
  }

  focus(sessionId: string): void {
    this.entries.get(sessionId)?.term.focus();
  }

  /** Types file paths into the session, as a paste (so Claude Code attaches images among them). */
  pastePaths(sessionId: string, paths: string[]): void {
    const entry = this.entries.get(sessionId);
    if (!entry || paths.length === 0) return;
    entry.term.paste(formatPaths(paths, window.reframe.platform));
    entry.term.focus();
  }

  private create(sessionId: string): Entry {
    const term = new Terminal({
      allowProposedApi: true,
      fontFamily: FONT,
      fontSize: 13,
      lineHeight: 1.1,
      cursorBlink: true,
      scrollback: 5000,
      theme: THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = '11';
    term.loadAddon(new WebLinksAddon((_event, uri) => window.reframe.openExternal(uri)));
    term.attachCustomKeyEventHandler((event) =>
      handleClipboardKeys(term, event, (data) => this.send(entry, { type: 'input', data })),
    );

    const element = document.createElement('div');
    element.className = 'terminal-host';
    const entry: Entry = {
      sessionId,
      term,
      fit,
      element,
      port: null,
      observer: new ResizeObserver(() => this.scheduleFit(entry)),
      opened: false,
      lastSize: null,
      frame: 0,
    };
    term.onData((data) => this.send(entry, { type: 'input', data }));
    this.entries.set(sessionId, entry);
    window.reframe.attachTerminal(sessionId);
    return entry;
  }

  private connect(sessionId: string, port: MessagePort): void {
    const entry = this.entries.get(sessionId);
    if (!entry) {
      port.close();
      return;
    }
    entry.port?.close();
    entry.port = port;
    entry.lastSize = null;
    port.onmessage = (event: MessageEvent<TerminalHostMessage>) => {
      const message = event.data;
      if (message.type === 'snapshot') {
        entry.term.reset();
        entry.term.resize(message.cols, message.rows);
        entry.term.write(message.data, () => this.scheduleFit(entry));
      } else if (message.type === 'data') {
        entry.term.write(message.data);
      }
    };
    port.start();
  }

  private scheduleFit(entry: Entry): void {
    cancelAnimationFrame(entry.frame);
    entry.frame = requestAnimationFrame(() => {
      if (!entry.opened || !entry.element.isConnected || entry.element.clientWidth === 0) return;
      const size = entry.fit.proposeDimensions();
      if (!size || !Number.isFinite(size.cols) || !Number.isFinite(size.rows)) return;
      if (size.cols !== entry.term.cols || size.rows !== entry.term.rows) entry.term.resize(size.cols, size.rows);
      if (entry.lastSize?.cols === size.cols && entry.lastSize.rows === size.rows) return;
      entry.lastSize = { cols: size.cols, rows: size.rows };
      this.send(entry, { type: 'resize', cols: size.cols, rows: size.rows });
    });
  }

  private send(entry: Entry, message: TerminalViewMessage): void {
    entry.port?.postMessage(message);
  }
}

/**
 * Ctrl+C copies when text is selected (otherwise it interrupts claude). Ctrl+V pastes text; files copied in a
 * file manager come in as their paths, and an image is left to Claude Code, which reads it from the clipboard on
 * its own paste key.
 */
function handleClipboardKeys(term: Terminal, event: KeyboardEvent, input: (data: string) => void): boolean {
  if (event.type !== 'keydown' || !event.ctrlKey || event.altKey || event.metaKey) return true;
  const key = event.key.toLowerCase();
  if (key === 'c' && (event.shiftKey || term.hasSelection())) {
    if (term.hasSelection()) void navigator.clipboard.writeText(term.getSelection());
    term.clearSelection();
    return false;
  }
  // Everything pasted comes through here (not the browser's own paste), so text, files and images agree.
  if (key === 'v') {
    event.preventDefault();
    void window.reframe.readClipboard().then((clipboard) => {
      if (clipboard.text) term.paste(clipboard.text);
      else if (clipboard.files.length > 0) term.paste(formatPaths(clipboard.files, window.reframe.platform));
      else if (clipboard.image) input(imagePasteKey(window.reframe.platform));
    });
    return false;
  }
  return true;
}
