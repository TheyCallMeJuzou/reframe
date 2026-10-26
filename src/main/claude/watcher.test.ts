import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ClaudeWatcher, type Changes, type Watch } from './watcher';

const temps: string[] = [];
let watcher: ClaudeWatcher | null = null;
afterEach(() => {
  watcher?.stop();
  vi.useRealTimers();
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function home(...folders: string[]): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-watcher-'));
  temps.push(dir);
  for (const folder of folders) fs.mkdirSync(path.join(dir, folder));
  return dir;
}

/** A stand-in for fs.watch: the listeners by folder, to fire events by hand. */
function fakeWatch() {
  const listeners = new Map<string, (event: string, filename: string | null) => void>();
  const watch: Watch = (dir, _options, listener) => {
    listeners.set(path.basename(dir), listener);
    return { close: () => listeners.delete(path.basename(dir)), on: () => ({}) as never };
  };
  return {
    watch,
    fire: (folder: string, filename: string | null) =>
      listeners.get(folder)?.(filename ? 'change' : 'rename', filename),
    listeners,
  };
}

describe('ClaudeWatcher', () => {
  it('tells changes in batches, with the project folders they touched', () => {
    vi.useFakeTimers();
    const fake = fakeWatch();
    const told: Changes[] = [];
    watcher = new ClaudeWatcher(home('sessions', 'projects'), { watch: fake.watch, batchMs: 100 });
    watcher.on('changed', (changes) => told.push(changes));
    watcher.start();
    vi.advanceTimersByTime(100);
    // On start, both count as changed (what they hold already).
    expect(told).toEqual([{ registry: true, projects: 'all' }]);

    fake.fire('projects', path.join('C--work', 'a.jsonl'));
    fake.fire('projects', path.join('C--web', 'b.jsonl'));
    fake.fire('sessions', '123.abc.key');
    vi.advanceTimersByTime(100);
    expect(told[1]).toEqual({ registry: false, projects: new Set(['C--work', 'C--web']) });

    fake.fire('sessions', '123.json');
    fake.fire('projects', null);
    vi.advanceTimersByTime(100);
    expect(told[2]).toEqual({ registry: true, projects: 'all' });
  });

  it('starts watching a folder once it appears', () => {
    vi.useFakeTimers();
    const fake = fakeWatch();
    const dir = home();
    watcher = new ClaudeWatcher(dir, { watch: fake.watch, batchMs: 10, lookMs: 50 });
    watcher.start();
    expect(fake.listeners.size).toBe(0);
    fs.mkdirSync(path.join(dir, 'sessions'));
    vi.advanceTimersByTime(50);
    expect([...fake.listeners.keys()]).toEqual(['sessions']);
  });

  it('looks at a folder that cannot be watched every little while instead', () => {
    vi.useFakeTimers();
    const told: Changes[] = [];
    const broken: Watch = () => {
      throw new Error('no watching here');
    };
    watcher = new ClaudeWatcher(home('projects'), { watch: broken, batchMs: 10, lookMs: 50, fallbackMs: 1000 });
    watcher.on('changed', (changes) => told.push(changes));
    watcher.start();
    vi.advanceTimersByTime(500);
    expect(told).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(told).toHaveLength(2);
    expect(told[1]).toEqual({ registry: false, projects: 'all' });
  });

  it('notices real changes with fs.watch', async () => {
    const dir = home('sessions', 'projects');
    fs.mkdirSync(path.join(dir, 'projects', 'C--work'));
    const told: Changes[] = [];
    watcher = new ClaudeWatcher(dir, { batchMs: 50 });
    watcher.on('changed', (changes) => told.push(changes));
    watcher.start();
    await expect.poll(() => told.length).toBe(1);
    fs.writeFileSync(path.join(dir, 'projects', 'C--work', 'a.jsonl'), '{}\n');
    await expect.poll(() => told.length, { timeout: 5000 }).toBeGreaterThan(1);
    expect(told.slice(1).some((c) => c.projects === 'all' || c.projects.has('C--work'))).toBe(true);
  });
});
