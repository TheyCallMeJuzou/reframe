// Follows title-bar drags of other apps' windows (Windows). When a terminal window that hosts a claude
// running outside reframe is dragged over one of our windows, it reports the pointer position (so the window
// can show drop zones) and, on release, the drop with the sessions found in that terminal window.
import { EventEmitter } from 'node:events';
import { screen } from 'electron';
import type { RegistryEntry } from './claude/registry';
import { consoleHelper } from './console';
import { isTerminalHost, pidsInWindow } from './foreign-match';
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
  sessions: RegistryEntry[] | null;
  probe: Promise<RegistryEntry[]>;
  timer: NodeJS.Timeout;
  hovered: string | null;
}

const TRACK_MS = 50;

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
    const pids = pidsInWindow(hwnd, result.owners ?? []);
    return external.filter((e) => pids.includes(e.pid));
  }

  private begin(hwnd: string): void {
    this.cancel();
    // Moving windows is frequent: only terminals, and only while some claude runs outside reframe, cost more.
    const external = this.deps.externalSessions();
    if (external.length === 0) return;
    const host = this.deps.win32.imageName(this.deps.win32.processId(hwnd));
    if (!isTerminalHost(host)) return;
    const probe = this.sessionsIn(hwnd, external);
    const drag: Drag = {
      hwnd,
      sessions: null,
      probe,
      hovered: null,
      timer: setInterval(() => this.track(drag), TRACK_MS),
    };
    void probe.then((sessions) => (drag.sessions = sessions));
    this.drag = drag;
  }

  private track(drag: Drag): void {
    if (!drag.sessions?.length) return; // not (yet) known to hold a claude
    const point = screen.getCursorScreenPoint();
    const over = this.deps.windowAt(point) ?? null;
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
    const sessions = await drag.probe;
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
