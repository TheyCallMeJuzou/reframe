// Loads and saves state.json. Writes are atomic (temp file + rename) and the previous good file is
// kept as state.json.bak, so a crash mid-write or a corrupt file never loses the layout.
import fs from 'node:fs';
import path from 'node:path';
import { appStateSchema, defaultState, type AppState } from '@shared/state';

export type LoadSource = 'file' | 'backup' | 'default';

export class StateStore {
  private timer: NodeJS.Timeout | null = null;
  private pending: AppState | null = null;

  constructor(readonly file: string) {}

  get backupFile(): string {
    return `${this.file}.bak`;
  }

  load(): { state: AppState; source: LoadSource } {
    const fromFile = readState(this.file);
    if (fromFile) return { state: fromFile, source: 'file' };
    const fromBackup = readState(this.backupFile);
    if (fromBackup) return { state: fromBackup, source: 'backup' };
    return { state: defaultState(), source: 'default' };
  }

  /** Writes immediately (and cancels a pending debounced write). */
  save(state: AppState): void {
    this.cancel();
    writeAtomic(this.file, this.backupFile, JSON.stringify(state, null, 2));
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
}

function readState(file: string): AppState | null {
  try {
    const parsed = appStateSchema.safeParse(JSON.parse(fs.readFileSync(file, 'utf8')));
    return parsed.success ? parsed.data : null;
  } catch {
    return null;
  }
}

function writeAtomic(file: string, backup: string, content: string): void {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  const fd = fs.openSync(temp, 'w');
  try {
    fs.writeSync(fd, content);
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }
  if (readState(file)) fs.copyFileSync(file, backup);
  renameWithRetry(temp, file);
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
