// Loads and saves state.json. Writes are atomic (temp file + rename) and the previous good file is
// kept as state.json.bak, so a crash mid-write or a corrupt file never loses the layout.
// A file that cannot be used is never written over: one from a newer reframe is left alone (nothing is written at
// all, so going back to that version keeps everything), any other is set aside as state.json.rejected-<time>.
import fs from 'node:fs';
import path from 'node:path';
import { appStateSchema, defaultState, STATE_VERSION, type AppState } from '@shared/state';

export type LoadSource = 'file' | 'backup' | 'default' | 'newer';

export interface LoadResult {
  state: AppState;
  source: LoadSource;
  /** The file could not be used and was set aside under this name, for this reason. */
  rejected?: { file: string; reason: string };
}

/** How many set-aside files are kept. */
const KEEP_REJECTED = 5;

export class StateStore {
  private timer: NodeJS.Timeout | null = null;
  private pending: AppState | null = null;
  /** Size and time of the file as this store last loaded or wrote it: while they match, it is known to be good. */
  private known: { size: number; mtimeMs: number } | null = null;
  /** The file belongs to a newer reframe: nothing is written. */
  private readOnly = false;

  constructor(readonly file: string) {}

  get backupFile(): string {
    return `${this.file}.bak`;
  }

  load(): LoadResult {
    const fromFile = readFile(this.file);
    if (fromFile.state) {
      this.known = stat(this.file);
      return { state: fromFile.state, source: 'file' };
    }
    if (fromFile.newer) {
      this.readOnly = true;
      return { state: defaultState(), source: 'newer' };
    }
    const rejected = fromFile.reason ? { file: this.setAside(), reason: fromFile.reason } : undefined;
    const fromBackup = readFile(this.backupFile);
    if (fromBackup.newer) {
      // The backup is a newer reframe's as well: it must not be written over either.
      this.readOnly = true;
      return { state: defaultState(), source: 'newer', ...(rejected ? { rejected } : {}) };
    }
    if (fromBackup.state) return { state: fromBackup.state, source: 'backup', ...(rejected ? { rejected } : {}) };
    return { state: defaultState(), source: 'default', ...(rejected ? { rejected } : {}) };
  }

  /** Writes immediately (and cancels a pending debounced write). */
  save(state: AppState): void {
    this.cancel();
    if (this.readOnly) return;
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    const temp = `${this.file}.${process.pid}.tmp`;
    const fd = fs.openSync(temp, 'w');
    try {
      fs.writeSync(fd, JSON.stringify(state, null, 2));
      fs.fsyncSync(fd);
    } finally {
      fs.closeSync(fd);
    }
    if (this.isGood()) fs.copyFileSync(this.file, this.backupFile);
    renameWithRetry(temp, this.file);
    this.known = stat(this.file);
  }

  /** Coalesces frequent saves (window moves, resizes) into one write. */
  saveSoon(state: AppState, delayMs = 500): void {
    this.pending = state;
    if (this.timer) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      const next = this.pending;
      this.pending = null;
      if (next) this.save(next);
    }, delayMs);
  }

  flush(): void {
    const next = this.pending;
    if (next) this.save(next);
  }

  private cancel(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.pending = null;
  }

  /** Whether the file on disk is a good one, worth keeping as the backup; read again only if it changed meanwhile. */
  private isGood(): boolean {
    const now = stat(this.file);
    if (!now) return false;
    if (this.known && now.size === this.known.size && now.mtimeMs === this.known.mtimeMs) return true;
    return !!readFile(this.file).state;
  }

  /** Moves the unusable file out of the way (beyond KEEP_REJECTED such files, the oldest go); its new name. */
  private setAside(): string {
    const dir = path.dirname(this.file);
    const prefix = `${path.basename(this.file)}.rejected-`;
    const stamp = new Date().toISOString().replace(/[-:]/g, '').replace('T', '-').slice(0, 15);
    // Within the same second, numbered on from the highest number there (never one freed by trimming), so that the
    // names sort by age.
    const same = fs.readdirSync(dir).filter((name) => name.startsWith(prefix + stamp));
    const highest = Math.max(0, ...same.map((name) => Number(/-(\d{3})$/.exec(name)?.[1] ?? 1)));
    const target = path.join(dir, prefix + stamp + (same.length > 0 ? `-${String(highest + 1).padStart(3, '0')}` : ''));
    try {
      fs.renameSync(this.file, target);
    } catch {
      // Held open by something: it stays where it is, and the next save will not take it for the backup either.
      return this.file;
    }
    const old = fs
      .readdirSync(dir)
      .filter((name) => name.startsWith(prefix))
      .sort()
      .slice(0, -KEEP_REJECTED);
    for (const name of old) fs.rmSync(path.join(dir, name), { force: true });
    return target;
  }
}

/** The state in a file, or why there is none: missing (no reason), from a newer reframe, or unusable. */
function readFile(file: string): { state?: AppState; newer?: boolean; reason?: string } {
  let text: string;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return {};
  }
  let json: unknown;
  try {
    json = JSON.parse(text);
  } catch (error) {
    return { reason: `not JSON (${(error as Error).message})` };
  }
  const version = (json as { version?: unknown } | null)?.version;
  if (typeof version === 'number' && version > STATE_VERSION) return { newer: true };
  const parsed = appStateSchema.safeParse(json);
  if (parsed.success) return { state: parsed.data };
  const issues = parsed.error.issues.slice(0, 5).map((i) => `${i.path.join('.') || '(file)'}: ${i.message}`);
  return { reason: issues.join('; ') };
}

function stat(file: string): { size: number; mtimeMs: number } | null {
  try {
    const { size, mtimeMs } = fs.statSync(file);
    return { size, mtimeMs };
  } catch {
    return null;
  }
}

// On Windows a scanner or indexer can hold the target open for a moment.
function renameWithRetry(from: string, to: string): void {
  for (let attempt = 0; ; attempt++) {
    try {
      fs.renameSync(from, to);
      return;
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (attempt >= 4 || (code !== 'EPERM' && code !== 'EBUSY' && code !== 'EACCES')) throw error;
      const until = Date.now() + 20 * (attempt + 1);
      while (Date.now() < until) {
        // short synchronous back-off; saves must not interleave
      }
    }
  }
}
