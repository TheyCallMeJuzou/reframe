import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { cleanUp, DROPS_KEEP_MS, olderThan, strayHookFiles } from './cleanup';

const temps: string[] = [];
afterEach(() => {
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe('cleanup', () => {
  it('picks entries older than the limit, and hook files of sessions that are gone', () => {
    expect(
      olderThan(
        [
          { name: 'old', mtimeMs: 0 },
          { name: 'new', mtimeMs: 900 },
        ],
        1000,
        500,
      ),
    ).toEqual(['old']);
    expect(strayHookFiles(['a.json', 'b.json', 'notes.txt'], new Set(['a']))).toEqual(['b.json']);
  });

  it('removes old drops and stray hook files, and nothing else', async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-cleanup-'));
    temps.push(root);
    const drops = path.join(root, 'drops');
    const hooks = path.join(root, 'hooks');
    for (const name of ['old', 'new']) {
      fs.mkdirSync(path.join(drops, name), { recursive: true });
      fs.writeFileSync(path.join(drops, name, 'image.png'), '');
    }
    const longAgo = new Date(Date.now() - DROPS_KEEP_MS - 60_000);
    fs.utimesSync(path.join(drops, 'old'), longAgo, longAgo);
    fs.mkdirSync(hooks);
    fs.writeFileSync(path.join(hooks, 'kept.json'), '{}');
    fs.writeFileSync(path.join(hooks, 'gone.json'), '{}');

    expect(await cleanUp(drops, hooks, new Set(['kept']))).toEqual({ drops: 1, hooks: 1 });
    expect(fs.readdirSync(drops)).toEqual(['new']);
    expect(fs.readdirSync(hooks)).toEqual(['kept.json']);
    // Folders that are not there are nothing to clean.
    expect(await cleanUp(path.join(root, 'none'), path.join(root, 'none'), new Set())).toEqual({ drops: 0, hooks: 0 });
  });
});
