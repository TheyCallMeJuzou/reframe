import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { defaultState, type AppState } from '@shared/state';
import { StateStore } from './store';

const temps: string[] = [];
function store(): StateStore {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-store-'));
  temps.push(dir);
  return new StateStore(path.join(dir, 'state.json'));
}
afterEach(() => {
  vi.useRealTimers();
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const withFolder = (folder: string): AppState => ({ ...defaultState(), recentFolders: [folder] });

describe('StateStore', () => {
  it('gives files from before preferences existed the default preferences', () => {
    const s = store();
    const { preferences: _preferences, ...old } = defaultState();
    fs.writeFileSync(s.file, JSON.stringify(old));
    expect(s.load()).toMatchObject({ source: 'file', state: { preferences: { nameFromFolder: true } } });
  });

  it('starts from defaults without a file', () => {
    const { state, source } = store().load();
    expect(source).toBe('default');
    expect(state.windows).toHaveLength(1);
  });

  it('round-trips state and keeps the previous version as backup', () => {
    const s = store();
    s.save(withFolder('one'));
    s.save(withFolder('two'));
    expect(s.load()).toMatchObject({ source: 'file', state: { recentFolders: ['two'] } });
    expect(JSON.parse(fs.readFileSync(s.backupFile, 'utf8')).recentFolders).toEqual(['one']);
    expect(fs.readdirSync(path.dirname(s.file)).filter((f) => f.endsWith('.tmp'))).toEqual([]);
  });

  it('falls back to the backup when the file is corrupt or invalid', () => {
    const s = store();
    s.save(withFolder('one'));
    s.save(withFolder('two'));
    fs.writeFileSync(s.file, '{ "version": 1, "sessions": ');
    expect(s.load()).toMatchObject({ source: 'backup', state: { recentFolders: ['one'] } });
    fs.writeFileSync(s.file, JSON.stringify({ version: 1, sessions: 'nope' }));
    expect(s.load().source).toBe('backup');
  });

  it('sets an unusable file aside instead of writing over it', () => {
    const s = store();
    s.save(withFolder('one'));
    s.save(withFolder('two'));
    const broken = JSON.stringify({ version: 1, sessions: 'nope' });
    fs.writeFileSync(s.file, broken);
    const loaded = s.load();
    expect(loaded).toMatchObject({ source: 'backup', state: { recentFolders: ['one'] } });
    expect(loaded.rejected?.reason).toContain('sessions');
    expect(path.basename(loaded.rejected!.file)).toMatch(/^state\.json\.rejected-\d{8}-\d{6}$/);
    // Saved twice, as a start and a window move do: the set-aside file and the backup's content survive.
    s.save(loaded.state);
    s.save(withFolder('three'));
    expect(fs.readFileSync(loaded.rejected!.file, 'utf8')).toBe(broken);
    expect(JSON.parse(fs.readFileSync(s.backupFile, 'utf8')).recentFolders).toEqual(['one']);
  });

  it('sets aside an unusable file without a backup too, and keeps only the last five', () => {
    const s = store();
    for (let i = 0; i < 7; i++) {
      fs.writeFileSync(s.file, `garbage ${i}`);
      expect(s.load()).toMatchObject({ source: 'default', rejected: { reason: expect.stringContaining('not JSON') } });
    }
    const kept = fs.readdirSync(path.dirname(s.file)).filter((f) => f.includes('.rejected-'));
    expect(kept).toHaveLength(5);
    const contents = kept.map((f) => fs.readFileSync(path.join(path.dirname(s.file), f), 'utf8')).sort();
    expect(contents).toEqual(['garbage 2', 'garbage 3', 'garbage 4', 'garbage 5', 'garbage 6']);
  });

  it('leaves a file from a newer reframe alone and writes nothing', () => {
    const s = store();
    const newer = JSON.stringify({ version: 2, sessions: {}, somethingNew: true });
    fs.writeFileSync(s.file, newer);
    expect(s.load()).toMatchObject({ source: 'newer' });
    s.save(withFolder('one'));
    s.saveSoon(withFolder('two'));
    s.flush();
    expect(fs.readFileSync(s.file, 'utf8')).toBe(newer);
    expect(fs.readdirSync(path.dirname(s.file))).toEqual(['state.json']);
  });

  it('does not overwrite a good backup with a broken file', () => {
    const s = store();
    s.save(withFolder('one'));
    s.save(withFolder('two'));
    fs.writeFileSync(s.file, 'garbage');
    s.save(withFolder('three'));
    expect(JSON.parse(fs.readFileSync(s.backupFile, 'utf8')).recentFolders).toEqual(['one']);
  });

  it('coalesces debounced saves and flushes on demand', () => {
    vi.useFakeTimers();
    const s = store();
    s.saveSoon(withFolder('a'));
    s.saveSoon(withFolder('b'));
    expect(fs.existsSync(s.file)).toBe(false);
    vi.advanceTimersByTime(600);
    expect(s.load().state.recentFolders).toEqual(['b']);
    s.saveSoon(withFolder('c'));
    s.flush();
    expect(s.load().state.recentFolders).toEqual(['c']);
  });
});
