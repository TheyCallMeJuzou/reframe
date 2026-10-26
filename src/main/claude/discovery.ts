// Finds Claude Code sessions outside reframe: running ones (session registry) and past ones (transcripts
// in ~/.claude/projects/<project>/<session id>.jsonl). Transcripts can be megabytes, so only their start
// and end are read, and results are cached by modification time.
import fs from 'node:fs';
import path from 'node:path';
import { isRunningEntry, parseRegistryEntry, type RegistryEntry } from './registry';

export interface RunningExclusion {
  pids: Set<number>;
  sessionIds: Set<string>;
}

const REGISTRY_FILE = /^\d+\.json$/;

/** A registry file's entry, unless it is one to leave out; null also for a file being written or removed now. */
function registryEntry(text: string, exclude: RunningExclusion): RegistryEntry | null {
  try {
    const entry = parseRegistryEntry(JSON.parse(text));
    return entry && !exclude.pids.has(entry.pid) && !exclude.sessionIds.has(entry.sessionId) ? entry : null;
  } catch {
    return null;
  }
}

/** Claudes that run on this system. */
export function listRunning(
  home: string,
  exclude: RunningExclusion,
  alive: (entry: RegistryEntry) => boolean = isRunningEntry,
): RegistryEntry[] {
  let files: string[];
  try {
    files = fs.readdirSync(path.join(home, 'sessions')).filter((f) => REGISTRY_FILE.test(f));
  } catch {
    return [];
  }
  return files
    .map((file) => {
      try {
        return registryEntry(fs.readFileSync(path.join(home, 'sessions', file), 'utf8'), exclude);
      } catch {
        return null;
      }
    })
    .filter((entry): entry is RegistryEntry => !!entry && alive(entry));
}

/** The same without blocking, for a WSL distribution's folder (`alive`: asked through its /proc there). */
export async function listRunningAsync(
  home: string,
  exclude: RunningExclusion,
  alive: (entry: RegistryEntry) => Promise<boolean>,
): Promise<RegistryEntry[]> {
  let files: string[];
  try {
    files = (await fs.promises.readdir(path.join(home, 'sessions'))).filter((f) => REGISTRY_FILE.test(f));
  } catch {
    return [];
  }
  const running: RegistryEntry[] = [];
  for (const file of files) {
    const text = await fs.promises.readFile(path.join(home, 'sessions', file), 'utf8').catch(() => '');
    const entry = registryEntry(text, exclude);
    if (entry && (await alive(entry))) running.push(entry);
  }
  return running;
}

export interface TranscriptInfo {
  sessionId: string;
  modifiedAt: number;
  cwd?: string;
  title?: string;
}

const HEAD_BYTES = 64 * 1024;
const TAIL_BYTES = 256 * 1024;
const TITLE_LENGTH = 90;

type Entry = Record<string, unknown> & { type?: string };

/** cwd and a title from a transcript's first and last lines. Null when it holds no prompt. */
export async function readTranscriptInfo(
  file: string,
): Promise<Omit<TranscriptInfo, 'sessionId' | 'modifiedAt'> | null> {
  const handle = await fs.promises.open(file, 'r');
  let head: string;
  let tail = '';
  try {
    const { size } = await handle.stat();
    head = await readChunk(handle, 0, Math.min(size, HEAD_BYTES));
    if (size > HEAD_BYTES) {
      const start = Math.max(HEAD_BYTES, size - TAIL_BYTES);
      tail = await readChunk(handle, start, size - start);
      tail = tail.slice(tail.indexOf('\n') + 1); // drop the partial first line
    }
  } finally {
    await handle.close();
  }
  const entries = [...parseLines(head), ...parseLines(tail)];
  const last = (type: string, field: string) => {
    for (let i = entries.length - 1; i >= 0; i--) {
      const value = entries[i]!.type === type ? entries[i]![field] : undefined;
      if (typeof value === 'string' && value.trim()) return value.trim();
    }
    return undefined;
  };
  const firstPrompt = entries
    .filter((e) => e.type === 'user')
    .map((e) => (e.message as { content?: unknown } | undefined)?.content)
    .find((c): c is string => typeof c === 'string' && !c.startsWith('<') && c.trim() !== '');
  const lastPrompt = last('last-prompt', 'lastPrompt');
  if (!firstPrompt && !lastPrompt) return null;
  const cwd = entries.find((e) => e.type === 'user' && typeof e.cwd === 'string')?.cwd as string | undefined;
  const title = last('agent-name', 'agentName') ?? last('ai-title', 'aiTitle') ?? lastPrompt ?? firstPrompt;
  return { cwd, title: title && truncate(title.replace(/\s+/g, ' ').trim(), TITLE_LENGTH) };
}

async function readChunk(handle: fs.promises.FileHandle, position: number, length: number): Promise<string> {
  const buffer = Buffer.alloc(length);
  await handle.read(buffer, 0, length, position);
  return buffer.toString('utf8');
}

function parseLines(text: string): Entry[] {
  const out: Entry[] = [];
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    try {
      const value: unknown = JSON.parse(line);
      if (value && typeof value === 'object') out.push(value as Entry);
    } catch {
      // a cut-off line at a chunk boundary
    }
  }
  return out;
}

const truncate = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** Past sessions, newest first, with search and paging. */
export class TranscriptIndex {
  private cache = new Map<string, { modifiedAt: number; info: TranscriptInfo | null }>();

  constructor(private home: string) {}

  async list(options: { query?: string; limit: number; exclude: Set<string> }): Promise<{
    items: TranscriptInfo[];
    hasMore: boolean;
  }> {
    const files = (await this.files()).filter((f) => !options.exclude.has(f.sessionId));
    const query = options.query?.trim().toLowerCase();
    const items: TranscriptInfo[] = [];
    for (const file of files) {
      const info = await this.info(file);
      if (!info) continue;
      if (query && ![info.title, info.cwd].some((v) => v?.toLowerCase().includes(query))) continue;
      if (items.length === options.limit) return { items, hasMore: true };
      items.push(info);
    }
    return { items, hasMore: false };
  }

  /** One past session by id, e.g. when it is opened from the list. */
  async find(sessionId: string): Promise<TranscriptInfo | null> {
    const file = (await this.files()).find((f) => f.sessionId === sessionId);
    return file ? this.info(file) : null;
  }

  /** The transcripts, newest first; read without blocking (in WSL they are behind a network share). */
  private async files(): Promise<{ file: string; sessionId: string; modifiedAt: number }[]> {
    const projects = path.join(this.home, 'projects');
    const out: { file: string; sessionId: string; modifiedAt: number }[] = [];
    let dirs: string[];
    try {
      dirs = await fs.promises.readdir(projects);
    } catch {
      return out;
    }
    for (const dir of dirs) {
      let names: string[];
      try {
        names = (await fs.promises.readdir(path.join(projects, dir))).filter((n) => n.endsWith('.jsonl'));
      } catch {
        continue;
      }
      for (const name of names) {
        const file = path.join(projects, dir, name);
        try {
          const modifiedAt = (await fs.promises.stat(file)).mtimeMs;
          out.push({ file, sessionId: name.slice(0, -'.jsonl'.length), modifiedAt });
        } catch {
          // removed in the meantime
        }
      }
    }
    return out.sort((a, b) => b.modifiedAt - a.modifiedAt);
  }

  private async info(file: { file: string; sessionId: string; modifiedAt: number }): Promise<TranscriptInfo | null> {
    const cached = this.cache.get(file.file);
    if (cached && cached.modifiedAt === file.modifiedAt) return cached.info;
    let info: TranscriptInfo | null = null;
    try {
      const read = await readTranscriptInfo(file.file);
      info = read && { sessionId: file.sessionId, modifiedAt: file.modifiedAt, ...read };
    } catch {
      // unreadable right now; try again next time
    }
    this.cache.set(file.file, { modifiedAt: file.modifiedAt, info });
    return info;
  }
}
