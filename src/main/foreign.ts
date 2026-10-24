// Follows title-bar drags of other apps' windows (Windows). When a terminal window that hosts a claude
// running outside reframe is dragged over one of our windows, it reports the pointer position (so the window
// can show drop zones) and, on release, the drop with the sessions found in that terminal window.
import { EventEmitter } from 'node:events';
import { screen } from 'electron';
import type { RegistryEntry } from './claude/registry';
import { consoleHelper } from './console';
import { isTerminalHost, pidsForDrop, pidsInWindow } from './foreign-match';
import type { LogFn } from './log';
import type { Win32 } from './win32';

export interface Point {
  x: number;
  y: number;
}

export interface ForeignDrop {
  hwnd: string;
  title: string;
  windowId: string;
  point: Point;
  sessions: RegistryEntry[];
}

export interface ForeignWatcherDeps {
  win32: Win32;
  /** Our visible window under a screen point. */
  windowAt: (point: Point) => string | undefined;
  /** Claudes running outside reframe (Claude Code's session registry). */
  externalSessions: () => RegistryEntry[];
  log: LogFn;
}

type Events = {
  /** The pointer moved over one of our windows during such a drag; null when it left it again. */
  hover: [windowId: string, point: Point | null];
  drop: [ForeignDrop];
};

interface Drag {
  hwnd: string;
  /** Sessions found in the window so far; null until the first lookup is done. */
  sessions: RegistryEntry[] | null;
  probing: Promise<RegistryEntry[]> | null;
  lastProbe: number;
  timer: NodeJS.Timeout;
  hovered: string | null;
}

const TRACK_MS = 50;
const REPROBE_MS = 700;

export class ForeignWindowWatcher extends EventEmitter<Events> {
  private drag: Drag | null = null;

  constructor(private deps: ForeignWatcherDeps) {
    super();
  }

  start(): void {
    this.deps.win32.onMoveSize((phase, hwnd) => {
      if (phase === 'start') this.begin(hwnd);
      else void this.end(hwnd);
    });
  }

  /** Claude sessions whose console lives in terminal window `hwnd`. */
  async sessionsIn(hwnd: string, external = this.deps.externalSessions()): Promise<RegistryEntry[]> {
    if (external.length === 0) return [];
    const result = await consoleHelper({ type: 'owners', pids: external.map((e) => e.pid) });
    if (!result.ok) {
      this.deps.log('warn', 'could not map consoles to terminal windows', { error: result.error });
      return [];
    }
    const owners = result.owners ?? [];
    const pids = pidsForDrop(hwnd, owners, (window) => this.deps.win32.isWindow(window));
    if (pids.length > 0 && pidsInWindow(hwnd, owners).length === 0) {
      this.deps.log('info', 'matched a torn-out terminal tab by its vanished window', { pids });
    }
    return external.filter((e) => pids.includes(e.pid));
  }

  private begin(hwnd: string): void {
    this.cancel();
    // Moving windows is frequent: only terminals, and only while some claude runs outside reframe, cost more.
    const external = this.deps.externalSessions();
    if (external.length === 0) return;
    const host = this.deps.win32.imageName(this.deps.win32.processId(hwnd));
    if (!isTerminalHost(host)) return;
    this.deps.log('info', 'terminal window drag started', { host, claudesElsewhere: external.length });
    const drag: Drag = {
      hwnd,
      sessions: null,
      probing: null,
      lastProbe: 0,
      hovered: null,
      timer: setInterval(() => this.track(drag), TRACK_MS),
    };
    this.probe(drag);
    this.drag = drag;
  }

  private probe(drag: Drag): Promise<RegistryEntry[]> {
    if (!drag.probing) {
      drag.lastProbe = Date.now();
      drag.probing = this.sessionsIn(drag.hwnd).then((sessions) => {
        drag.sessions = sessions;
        drag.probing = null;
        return sessions;
      });
    }
    return drag.probing;
  }

  private track(drag: Drag): void {
    const point = screen.getCursorScreenPoint();
    const over = this.deps.windowAt(point) ?? null;
    if (!drag.sessions?.length) {
      // Windows Terminal may move a torn-out tab's console to its new window only after a moment: ask again.
      if (over && drag.sessions && Date.now() - drag.lastProbe > REPROBE_MS) void this.probe(drag);
      return;
    }
    if (drag.hovered && drag.hovered !== over) this.emit('hover', drag.hovered, null);
    drag.hovered = over;
    if (over) this.emit('hover', over, point);
  }

  private async end(hwnd: string): Promise<void> {
    const drag = this.drag;
    if (!drag || drag.hwnd !== hwnd) return;
    this.cancel();
    const point = screen.getCursorScreenPoint();
    const windowId = this.deps.windowAt(point);
    let sessions = drag.sessions ?? (await this.probe(drag));
    // Dropped on us with nothing found yet: one more look, now that the window has settled.
    if (windowId && sessions.length === 0) sessions = await this.sessionsIn(hwnd);
    this.deps.log('info', 'terminal window drag ended', { overReframe: !!windowId, sessions: sessions.length });
    if (!windowId || sessions.length === 0) return;
    this.emit('drop', { hwnd, title: this.deps.win32.title(hwnd), windowId, point, sessions });
  }

  private cancel(): void {
    const drag = this.drag;
    this.drag = null;
    if (!drag) return;
    clearInterval(drag.timer);
    if (drag.hovered) this.emit('hover', drag.hovered, null);
  }
}
