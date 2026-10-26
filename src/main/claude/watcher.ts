// Watches a Claude Code folder for what the session list shows: its session registry (claudes starting and ending)
// and its transcripts (sessions being used). Changes come in batches, with the project folders they touched, so the
// transcripts are listed again only there. A folder that is not there yet is looked for every couple of seconds; one
// that cannot be watched counts as changed every little while instead.
import { EventEmitter } from 'node:events';
import fs from 'node:fs';
import path from 'node:path';

export interface Changes {
  /** Something in the session registry changed. */
  registry: boolean;
  /** Project folders whose transcripts changed; 'all' when that is not known. */
  projects: Set<string> | 'all';
}

export type Watch = (
  dir: string,
  options: { recursive: boolean },
  listener: (event: string, filename: string | null) => void,
) => Pick<fs.FSWatcher, 'close' | 'on'>;

export interface WatcherOptions {
  watch?: Watch;
  /** How long changes are collected before they are told. */
  batchMs?: number;
  /** How often a missing (or not yet watched) folder is looked for. */
  lookMs?: number;
  /** How often a folder that cannot be watched counts as changed. */
  fallbackMs?: number;
}

type Folder = 'sessions' | 'projects';
const FOLDERS: { name: Folder; recursive: boolean }[] = [
  { name: 'sessions', recursive: false },
  { name: 'projects', recursive: true },
];

export class ClaudeWatcher extends EventEmitter<{ changed: [Changes] }> {
  private watchers = new Map<Folder, Pick<fs.FSWatcher, 'close'>>();
  private unwatchable = new Map<Folder, number>();
  private pending: Changes | null = null;
  private batch: NodeJS.Timeout | null = null;
  private look: NodeJS.Timeout | null = null;

  constructor(
    private home: string,
    private options: WatcherOptions = {},
  ) {
    super();
  }

  start(): void {
    this.attach();
    this.look = setInterval(() => this.attach(), this.options.lookMs ?? 2000);
    this.look.unref();
  }

  stop(): void {
    if (this.look) clearInterval(this.look);
    if (this.batch) clearTimeout(this.batch);
    this.look = this.batch = null;
    for (const watcher of this.watchers.values()) watcher.close();
    this.watchers.clear();
  }

  /** Starts watching the folders that are there and not watched yet. */
  private attach(): void {
    for (const { name, recursive } of FOLDERS) {
      if (this.watchers.has(name)) continue;
      const dir = path.join(this.home, name);
      if (!fs.existsSync(dir)) continue;
      const failedAt = this.unwatchable.get(name);
      if (failedAt !== undefined) {
        if (Date.now() - failedAt < (this.options.fallbackMs ?? 30_000)) continue;
        this.unwatchable.delete(name);
      }
      try {
        const watcher = (this.options.watch ?? fs.watch)(dir, { recursive }, (_event, filename) =>
          this.note(name, filename),
        );
        // Gone (the folder was removed, say): looked for again like a missing one.
        watcher.on('error', () => {
          watcher.close();
          this.watchers.delete(name);
          this.note(name, null);
        });
        this.watchers.set(name, watcher);
      } catch {
        this.unwatchable.set(name, Date.now());
      }
      // What it holds already: it may have just appeared, or it is looked at instead of watched.
      this.note(name, null);
    }
  }

  private note(folder: Folder, filename: string | null): void {
    // Claude Code keeps other files next to the registry's (<pid>.json) there.
    if (folder === 'sessions' && filename && !filename.endsWith('.json')) return;
    const pending = (this.pending ??= { registry: false, projects: new Set() });
    if (folder === 'sessions') pending.registry = true;
    else if (!filename) pending.projects = 'all';
    else if (pending.projects !== 'all') pending.projects.add(filename.split(/[\\/]/)[0]!);
    // Told a moment after the first change, however many follow, so a busy session does not hold it back.
    this.batch ??= setTimeout(() => {
      this.batch = null;
      const changes = this.pending!;
      this.pending = null;
      this.emit('changed', changes);
    }, this.options.batchMs ?? 1000);
  }
}
