// Reads Claude Code's per-process session registry (~/.claude/sessions/<pid>.json) and transcripts.
// The registry format is undocumented, so everything here is parsed defensively and kept in one place.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export interface RegistryEntry {
  pid: number;
  sessionId: string;
  cwd?: string;
  name?: string;
  /** Whether the user named the session ('user'), or Claude did ('auto', 'derived'). */
  nameSource?: string;
  status?: 'idle' | 'busy' | 'waiting';
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
  };
}

export function readRegistryEntry(home: string, pid: number): RegistryEntry | null {
  try {
    const entry = parseRegistryEntry(JSON.parse(fs.readFileSync(path.join(home, 'sessions', `${pid}.json`), 'utf8')));
    return entry && entry.pid === pid ? entry : null;
  } catch {
    return null;
  }
}

/**
 * The entry of a running claude by its session id, for one whose process id reframe does not know (in WSL, the
 * entry is named after the Linux process id).
 */
export function findRegistryEntry(home: string, sessionId: string): RegistryEntry | null {
  let files: string[];
  try {
    files = fs.readdirSync(path.join(home, 'sessions')).filter((f) => /^\d+\.json$/.test(f));
  } catch {
    return null;
  }
  for (const file of files) {
    const entry = readRegistryEntry(home, Number(file.slice(0, -'.json'.length)));
    if (entry?.sessionId === sessionId) return entry;
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
