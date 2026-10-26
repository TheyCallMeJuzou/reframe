// Reads Claude Code's per-process session registry (~/.claude/sessions/<pid>.json) and transcripts.
// The registry format is undocumented, so everything here is parsed defensively and kept in one place.
// The async readers are for a WSL distribution's Claude Code folder: it is behind a network share, which can take
// seconds to answer (a distribution that stopped is started by it) and must not hold up the main process.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { isAlive, processStart } from '../process';

export interface RegistryEntry {
  pid: number;
  sessionId: string;
  cwd?: string;
  name?: string;
  /** Whether the user named the session ('user'), or Claude did ('auto', 'derived'). */
  nameSource?: string;
  status?: 'idle' | 'busy' | 'waiting';
  /** When the process started (ms since 1970). */
  startedAt?: number;
  /** The process's start as the system tells it (on Windows its creation time). */
  procStart?: string;
}

const STATUSES = new Set(['idle', 'busy', 'waiting']);

/** Claude Code's config dir: CLAUDE_CONFIG_DIR if set, else ~/.claude. REFRAME_CLAUDE_HOME wins for tests. */
export function claudeHome(env: NodeJS.ProcessEnv = process.env): string {
  return env.REFRAME_CLAUDE_HOME || env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude');
}

export function parseRegistryEntry(value: unknown): RegistryEntry | null {
  if (!value || typeof value !== 'object') return null;
  const v = value as Record<string, unknown>;
  if (typeof v.pid !== 'number' || typeof v.sessionId !== 'string' || !v.sessionId) return null;
  return {
    pid: v.pid,
    sessionId: v.sessionId,
    cwd: typeof v.cwd === 'string' ? v.cwd : undefined,
    name: typeof v.name === 'string' && v.name ? v.name : undefined,
    nameSource: typeof v.nameSource === 'string' ? v.nameSource : undefined,
    status: typeof v.status === 'string' && STATUSES.has(v.status) ? (v.status as RegistryEntry['status']) : undefined,
    startedAt: typeof v.startedAt === 'number' ? v.startedAt : undefined,
    procStart: typeof v.procStart === 'string' && v.procStart ? v.procStart : undefined,
  };
}

/**
 * Whether the claude an entry names runs. A process that was killed leaves its entry behind, and Windows soon gives
 * its id to another program: where the system tells when a process started, that must be when this one did.
 */
export function isRunningEntry(entry: RegistryEntry, startOf: (pid: number) => string | null = processStart): boolean {
  if (!isAlive(entry.pid)) return false;
  const start = entry.procStart ? startOf(entry.pid) : null;
  return start === null || start === entry.procStart;
}

/** Removes the entry of a claude of reframe's that was killed (and could not remove it itself), once it is gone. */
export function forgetRegistryEntry(home: string, pid: number): void {
  if (isAlive(pid) || !readRegistryEntry(home, pid)) return;
  fs.rmSync(path.join(home, 'sessions', `${pid}.json`), { force: true });
}

export function readRegistryEntry(home: string, pid: number): RegistryEntry | null {
  try {
    const entry = parseRegistryEntry(JSON.parse(fs.readFileSync(path.join(home, 'sessions', `${pid}.json`), 'utf8')));
    return entry && entry.pid === pid ? entry : null;
  } catch {
    return null;
  }
}

export async function readRegistryEntryAsync(home: string, pid: number): Promise<RegistryEntry | null> {
  try {
    const text = await fs.promises.readFile(path.join(home, 'sessions', `${pid}.json`), 'utf8');
    const entry = parseRegistryEntry(JSON.parse(text));
    return entry && entry.pid === pid ? entry : null;
  } catch {
    return null;
  }
}

/**
 * The entry of a running claude by its session id, for one whose process id reframe does not know (in WSL, the
 * entry is named after the Linux process id). `alive` tells a running claude's entry from one a killed claude left
 * behind, which would otherwise be taken for the session's for good.
 */
export async function findRegistryEntry(
  home: string,
  sessionId: string,
  alive?: (entry: RegistryEntry) => Promise<boolean>,
): Promise<RegistryEntry | null> {
  let files: string[];
  try {
    files = (await fs.promises.readdir(path.join(home, 'sessions'))).filter((f) => /^\d+\.json$/.test(f));
  } catch {
    return null;
  }
  for (const file of files) {
    const entry = await readRegistryEntryAsync(home, Number(file.slice(0, -'.json'.length)));
    if (entry?.sessionId === sessionId && (!alive || (await alive(entry)))) return entry;
  }
  return null;
}

/** Whether Claude Code still has the transcript needed to resume a session. */
export function transcriptExists(home: string, sessionId: string): boolean {
  const projects = path.join(home, 'projects');
  let dirs: string[];
  try {
    dirs = fs.readdirSync(projects);
  } catch {
    return false;
  }
  return dirs.some((dir) => fs.existsSync(path.join(projects, dir, `${sessionId}.jsonl`)));
}

export async function transcriptExistsAsync(home: string, sessionId: string): Promise<boolean> {
  const projects = path.join(home, 'projects');
  let dirs: string[];
  try {
    dirs = await fs.promises.readdir(projects);
  } catch {
    return false;
  }
  // One after the other: a share does better with that than with hundreds of requests at once.
  for (const dir of dirs) {
    const found = await fs.promises.access(path.join(projects, dir, `${sessionId}.jsonl`)).then(
      () => true,
      () => false,
    );
    if (found) return true;
  }
  return false;
}
