import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import type { SessionRecord } from '@shared/state';
import { claudeArgs } from './args';
import { claudeEnv } from './env';
import { findRegistryEntry, parseRegistryEntry, readRegistryEntry, transcriptExists } from './registry';
import { resolveClaude } from './resolve';
import { hookSettings } from './settings';

const session = (patch: Partial<SessionRecord> = {}): SessionRecord => ({
  id: 'app-1',
  claudeSessionId: '11111111-2222-4333-8444-555555555555',
  cwd: 'C:/work/project',
  launch: {},
  lifecycle: 'open',
  createdAt: 0,
  lastActiveAt: 0,
  ...patch,
});

const temps: string[] = [];
function tempDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-test-'));
  temps.push(dir);
  return dir;
}
afterEach(() => {
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe('claudeEnv', () => {
  it('drops only the markers of a parent Claude session', () => {
    const env = claudeEnv(
      {
        PATH: '/bin',
        CLAUDECODE: '1',
        CLAUDE_PID: '42',
        CLAUDE_CODE_CHILD_SESSION: '1',
        CLAUDE_CODE_SESSION_ID: 'x',
        CLAUDE_CODE_ENTRYPOINT: 'cli',
        CLAUDE_CODE_USE_BEDROCK: '1',
        CLAUDE_CONFIG_DIR: '/cfg',
      },
      { REFRAME_SESSION: 'app-1' },
    );
    expect(env).toEqual({
      PATH: '/bin',
      CLAUDE_CODE_USE_BEDROCK: '1',
      CLAUDE_CONFIG_DIR: '/cfg',
      REFRAME_SESSION: 'app-1',
    });
  });
});

describe('claudeArgs', () => {
  it('starts a new session with a fixed id and options', () => {
    const s = session({
      name: 'api',
      launch: { model: 'opus', permissionMode: 'plan', worktree: true, worktreeName: 'feat' },
    });
    expect(claudeArgs(s, 'new', 'S.json')).toEqual([
      '--session-id',
      s.claudeSessionId,
      '--name',
      'api',
      '--model',
      'opus',
      '--permission-mode',
      'plan',
      '--worktree',
      'feat',
      '--settings',
      'S.json',
    ]);
  });

  it('keeps the name Claude Code was started with when the session is renamed in reframe', () => {
    expect(claudeArgs(session({ name: 'Shown', claudeName: 'api' }), 'resume', 'S.json')).toContain('api');
    expect(claudeArgs(session({ name: 'Shown', claudeName: null }), 'resume', 'S.json')).not.toContain('--name');
  });

  it('makes a worktree only on the first start, not when started fresh in it later', () => {
    expect(claudeArgs(session({ launch: { worktree: true } }), 'new')).toContain('--worktree');
    expect(claudeArgs(session({ launch: { worktree: true, worktreeCreated: true } }), 'new')).not.toContain(
      '--worktree',
    );
  });

  it('resumes without creating another worktree', () => {
    const s = session({ launch: { worktree: true } });
    expect(claudeArgs(s, 'resume', 'S.json')).toEqual(['--resume', s.claudeSessionId, '--settings', 'S.json']);
  });

  it('passes no settings when there are no hooks (WSL)', () => {
    const s = session();
    expect(claudeArgs(s, 'resume')).toEqual(['--resume', s.claudeSessionId]);
  });
});

describe('resolveClaude', () => {
  it('follows PATH and PATHEXT on Windows and unwraps npm cmd shims', () => {
    const files: Record<string, string> = {
      'C:\\npm\\claude.cmd': '@ECHO off\r\n"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe"   %*\r\n',
      'C:\\winget\\claude.exe': '',
    };
    const found = resolveClaude({
      platform: 'win32',
      env: { Path: 'C:\\npm;C:\\winget', PATHEXT: '.COM;.EXE;.BAT;.CMD' },
      isFile: (p) => p in files,
      readFile: (p) => files[p]!,
    });
    expect(found).toEqual({
      file: 'C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe',
      prefixArgs: [],
      env: {},
      source: 'C:\\npm\\claude.cmd',
    });
  });

  it('runs unknown cmd wrappers through cmd.exe', () => {
    const found = resolveClaude({
      platform: 'win32',
      env: { PATH: 'C:\\tools' },
      isFile: (p) => p === 'C:\\tools\\claude.cmd',
      readFile: () => 'some other launcher',
    });
    expect(found?.file).toBe('cmd.exe');
    expect(found?.prefixArgs).toEqual(['/d', '/s', '/c', 'C:\\tools\\claude.cmd']);
  });

  it('runs a .js override with the given node runner', () => {
    const runner = { file: 'electron.exe', env: { ELECTRON_RUN_AS_NODE: '1' } };
    const found = resolveClaude({ platform: 'win32', env: {}, override: 'C:/fake.mjs', nodeRunner: runner });
    expect(found).toEqual({
      file: 'electron.exe',
      prefixArgs: ['C:/fake.mjs'],
      env: { ELECTRON_RUN_AS_NODE: '1' },
      source: 'C:/fake.mjs',
    });
  });

  it('returns null when claude is missing', () => {
    expect(resolveClaude({ platform: 'win32', env: { PATH: 'C:\\x' }, isFile: () => false })).toBeNull();
  });
});

describe('registry', () => {
  it('parses known fields and ignores unknown status values', () => {
    expect(
      parseRegistryEntry({
        pid: 5,
        sessionId: 'abc',
        cwd: '/w',
        name: 'n',
        nameSource: 'user',
        status: 'busy',
        extra: 1,
      }),
    ).toEqual({ pid: 5, sessionId: 'abc', cwd: '/w', name: 'n', nameSource: 'user', status: 'busy' });
    expect(parseRegistryEntry({ pid: 5, sessionId: 'abc', status: 'thinking' })?.status).toBeUndefined();
    expect(parseRegistryEntry({ sessionId: 'abc' })).toBeNull();
    expect(parseRegistryEntry('nope')).toBeNull();
  });

  it('reads entries by pid and finds transcripts', () => {
    const home = tempDir();
    fs.mkdirSync(path.join(home, 'sessions'));
    fs.writeFileSync(path.join(home, 'sessions', '77.json'), JSON.stringify({ pid: 77, sessionId: 's-1' }));
    fs.writeFileSync(path.join(home, 'sessions', '78.json'), '{ broken');
    expect(readRegistryEntry(home, 77)?.sessionId).toBe('s-1');
    expect(readRegistryEntry(home, 78)).toBeNull();
    expect(readRegistryEntry(home, 79)).toBeNull();

    fs.mkdirSync(path.join(home, 'projects', 'C--work'), { recursive: true });
    fs.writeFileSync(path.join(home, 'projects', 'C--work', 's-1.jsonl'), '');
    expect(transcriptExists(home, 's-1')).toBe(true);
    expect(transcriptExists(home, 's-2')).toBe(false);
  });

  it('finds an entry by its session id when the process id is not known', () => {
    const home = tempDir();
    fs.mkdirSync(path.join(home, 'sessions'));
    fs.writeFileSync(path.join(home, 'sessions', '5740.json'), JSON.stringify({ pid: 5740, sessionId: 's-1' }));
    fs.writeFileSync(path.join(home, 'sessions', '5741.json'), JSON.stringify({ pid: 5741, sessionId: 's-2' }));
    fs.writeFileSync(path.join(home, 'sessions', 'notes.txt'), 's-2');
    expect(findRegistryEntry(home, 's-2')?.pid).toBe(5741);
    expect(findRegistryEntry(home, 's-3')).toBeNull();
    expect(findRegistryEntry(path.join(home, 'missing'), 's-1')).toBeNull();
  });
});

describe('hookSettings', () => {
  it('points every hook at the local server with the token', () => {
    const settings = hookSettings(4000, 'tok', 'app 1') as { hooks: Record<string, [{ hooks: [{ url: string }] }]> };
    expect(Object.keys(settings.hooks)).toEqual(['UserPromptSubmit', 'Stop', 'Notification']);
    expect(settings.hooks.Stop![0].hooks[0]).toEqual({
      type: 'http',
      url: 'http://127.0.0.1:4000/hook/app%201/Stop',
      headers: { 'X-Reframe-Token': 'tok' },
    });
  });
});
