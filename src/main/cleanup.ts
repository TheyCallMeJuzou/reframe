// What reframe leaves on disk and cleans up after a while: files dropped or pasted into sessions that had no path
// of their own (saved to the temp folder so a session gets a path for them; every image pasted into a WSL session
// is one), and the hook settings of sessions that are gone.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

/** Where dropped and pasted files are saved, one folder each. */
export const dropsDir = (): string => path.join(os.tmpdir(), 'reframe-drops');

/** How long a dropped file stays: Claude Code reads it when the prompt is sent, but the prompt may wait. */
export const DROPS_KEEP_MS = 7 * 24 * 60 * 60_000;

/** The entries older than `maxAgeMs`. */
export function olderThan(entries: { name: string; mtimeMs: number }[], now: number, maxAgeMs: number): string[] {
  return entries.filter((e) => now - e.mtimeMs > maxAgeMs).map((e) => e.name);
}

/** Hook settings files (<session id>.json) of sessions reframe no longer has. */
export function strayHookFiles(names: string[], sessionIds: ReadonlySet<string>): string[] {
  return names.filter((name) => name.endsWith('.json') && !sessionIds.has(name.slice(0, -'.json'.length)));
}

/** Removes old drops and stray hook files; how many of each. One that cannot go now (in use) goes next time. */
export async function cleanUp(
  drops: string,
  hooks: string,
  sessionIds: ReadonlySet<string>,
  now = Date.now(),
): Promise<{ drops: number; hooks: number }> {
  const entries = async (dir: string) => {
    const names = await fs.promises.readdir(dir).catch(() => [] as string[]);
    const stats = await Promise.all(
      names.map((name) =>
        fs.promises.stat(path.join(dir, name)).then(
          (s) => ({ name, mtimeMs: s.mtimeMs }),
          () => null,
        ),
      ),
    );
    return stats.filter((s): s is { name: string; mtimeMs: number } => !!s);
  };
  const remove = async (dir: string, names: string[]) => {
    const done = await Promise.all(
      names.map((name) =>
        fs.promises.rm(path.join(dir, name), { recursive: true, force: true }).then(
          () => true,
          () => false,
        ),
      ),
    );
    return done.filter(Boolean).length;
  };
  return {
    drops: await remove(drops, olderThan(await entries(drops), now, DROPS_KEEP_MS)),
    hooks: await remove(hooks, strayHookFiles(await fs.promises.readdir(hooks).catch(() => []), sessionIds)),
  };
}
