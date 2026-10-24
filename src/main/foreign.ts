// Follows title-bar drags of other apps' windows (Windows). When a terminal window that hosts a claude
// running outside reframe is dragged over one of our windows, it reports the pointer position (so the window
// can show drop zones) and, on release, the drop with the sessions found in that terminal window.
//
// Dragging a tab out of Windows Terminal is no window move: the terminal only opens a new window for the tab
// where it was released. A terminal window that appears over one of our windows, holding a claude that already
// ran before the window existed, counts as such a drop too. While such a tab is dragged over one of our
// windows, the window reports the pointer (a drag with no data types), so it can show drop zones as well.
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
/** How long after its creation a shown window counts as new. */
const NEW_WINDOW_MS = 10_000;
/** How long a pointer position reported by a window during a foreign drag stays the drop point. */
const POINTER_MS = 3_000;
/** When to look into a new terminal window: the moved tab's console may name its window only after a moment. */
const APPEARED_PROBES_MS = [0, 300, 800, 1500, 2500];
/** Without a pointer report for this long, a drag over our windows is over. */
const TAB_DRAG_IDLE_MS = 500;

interface TabDrag {
  windowId: string;
  /** Whether the terminal window the tab comes from holds a claude; null while that is being looked up. */
  holdsClaude: boolean | null;
  idle: NodeJS.Timeout;
}

export class ForeignWindowWatcher extends EventEmitter<Events> {
  private drag: Drag | null = null;
  private created = new Map<string, number>();
  private hosts = new Map<number, string | null>();
  private pointer: { point: Point; at: number } | null = null;
  private tabDrag: TabDrag | null = null;
  /** Test hook: the window a simulated tab drag comes from (instead of the window in front). */
  tabDragSource: string | null = null;

  constructor(private deps: ForeignWatcherDeps) {
    super();
  }

  start(): void {
    this.deps.win32.onMoveSize((phase, hwnd) => {
      if (phase === 'start') this.begin(hwnd);
      else void this.end(hwnd);
    });
    this.deps.win32.onTopLevel((phase, hwnd) => {
      const now = Date.now();
      if (phase === 'created') {
        for (const [window, at] of this.created) if (now - at > NEW_WINDOW_MS) this.created.delete(window);
        this.created.set(hwnd, now);
        return;
      }
      const createdAt = this.created.get(hwnd);
      this.created.delete(hwnd);
      if (createdAt !== undefined && now - createdAt <= NEW_WINDOW_MS) this.appeared(hwnd);
    });
  }

  /**
   * A window of ours reports the pointer (screen coordinates) during a drag it does not know, or null when the
   * drag left it. Windows Terminal's tab drags offer no data types; they come from the window in front.
   */
  notePointer(windowId: string, point: Point | null, types: string[]): void {
    if (!point || types.length > 0) return this.endTabDrag();
    this.pointer = { point, at: Date.now() };
    let drag = this.tabDrag;
    if (drag && drag.windowId !== windowId) {
      if (drag.holdsClaude) this.emit('hover', drag.windowId, null);
      drag.windowId = windowId;
    }
    if (!drag) {
      const created: TabDrag = {
        windowId,
        holdsClaude: null,
        idle: setTimeout(() => this.endTabDrag(), TAB_DRAG_IDLE_MS),
      };
      drag = this.tabDrag = created;
      const source = this.tabDragSource ?? this.deps.win32.foreground();
      const host = source ? this.hostOf(source) : null;
      if (!source || !isTerminalHost(host)) created.holdsClaude = false;
      else {
        void this.sessionsIn(source).then((sessions) => {
          if (this.tabDrag !== created) return;
          created.holdsClaude = sessions.length > 0;
          if (created.holdsClaude && this.pointer) this.emit('hover', created.windowId, this.pointer.point);
        });
      }
      this.deps.log('info', 'something from another app is dragged over reframe', { from: host ?? 'unknown' });
    }
    clearTimeout(drag.idle);
    drag.idle = setTimeout(() => this.endTabDrag(), TAB_DRAG_IDLE_MS);
    if (drag.holdsClaude) this.emit('hover', windowId, point);
  }

  private endTabDrag(): void {
    const drag = this.tabDrag;
    this.tabDrag = null;
    if (!drag) return;
    clearTimeout(drag.idle);
    if (drag.holdsClaude) this.emit('hover', drag.windowId, null);
  }

  /**
   * A new terminal window was shown: when it holds a claude that ran before and appeared over one of our
   * windows, it is a tab torn out over reframe. `point` (tests) stands in for the pointer.
   */
  appeared(hwnd: string, point?: Point): void {
    const host = this.hostOf(hwnd);
    if (!isTerminalHost(host)) return;
    const rect = this.deps.win32.rect(hwnd);
    if (!rect || rect.width === 0 || rect.height === 0) return; // helper windows of shells, never seen
    // Only claudes that already ran before the window appeared: one started in a new window was not dropped here.
    const external = this.deps.externalSessions();
    if (external.length === 0) return;
    const recent = this.pointer && Date.now() - this.pointer.at <= POINTER_MS ? this.pointer.point : null;
    const at = point ?? recent ?? screen.getCursorScreenPoint();
    const windowId = this.deps.windowAt(at);
    this.deps.log('info', 'new terminal window appeared', {
      host,
      overReframe: !!windowId,
      point: at,
      pointFrom: point ? 'test' : recent ? 'drag over reframe' : 'cursor',
      window: process.platform === 'win32' ? screen.screenToDipRect(null, rect) : rect,
    });
    if (!windowId) return;
    void this.probeAppeared(hwnd, external).then((sessions) => {
      this.deps.log('info', 'claudes in the new terminal window', { sessions: sessions.length });
      if (sessions.length > 0) {
        this.emit('drop', { hwnd, title: this.deps.win32.title(hwnd), windowId, point: at, sessions });
      }
    });
  }

  private async probeAppeared(hwnd: string, external: RegistryEntry[]): Promise<RegistryEntry[]> {
    let waited = 0;
    for (const delay of APPEARED_PROBES_MS) {
      await new Promise((resolve) => setTimeout(resolve, delay - waited));
      waited = delay;
      if (!this.deps.win32.isWindow(hwnd)) return [];
      const sessions = await this.sessionsIn(hwnd, external);
      if (sessions.length > 0) return sessions;
    }
    return [];
  }

  private hostOf(hwnd: string): string | null {
    const pid = this.deps.win32.processId(hwnd);
    if (!this.hosts.has(pid)) {
      if (this.hosts.size > 500) this.hosts.clear();
      this.hosts.set(pid, this.deps.win32.imageName(pid));
    }
    return this.hosts.get(pid) ?? null;
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
    const host = this.hostOf(hwnd);
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
