import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  decodeWslOutput,
  interopOf,
  isShellCommand,
  looksLikeClaude,
  parentOf,
  parentPidCommand,
  parseDistroList,
  parseProbe,
  WslDistros,
} from './distros';

describe('decodeWslOutput and parseDistroList', () => {
  it('reads the list in UTF-16 (older wsl.exe) and UTF-8', () => {
    expect(parseDistroList(decodeWslOutput(Buffer.from('Ubuntu\r\nDebian\r\n', 'utf16le')))).toEqual([
      'Ubuntu',
      'Debian',
    ]);
    expect(parseDistroList(decodeWslOutput(Buffer.from('\uFEFFUbuntu-24.04\r\n\r\n', 'utf8')))).toEqual([
      'Ubuntu-24.04',
    ]);
    expect(parseDistroList('')).toEqual([]);
  });
});

describe('looksLikeClaude', () => {
  it('knows the native claude, a link to it, and node running its script, by their command lines', () => {
    const argv = (...args: string[]) => args.join('\0');
    expect(looksLikeClaude(argv('/usr/local/bin/claude', '--resume', 'abc'))).toBe(true);
    expect(looksLikeClaude(argv('claude'))).toBe(true);
    expect(looksLikeClaude(argv('/home/ann/.local/share/claude/versions/2.1.309', '--session-id', 'x'))).toBe(true);
    expect(looksLikeClaude(argv('node', '/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js'))).toBe(true);
    expect(
      looksLikeClaude(
        argv(
          '/home/ann/.nvm/versions/node/v22.0.0/bin/node',
          '/home/ann/.local/share/pnpm/global/5/.pnpm/@anthropic-ai+claude-code@2.1.0/node_modules/@anthropic-ai/claude-code/cli.js',
        ),
      ),
    ).toBe(true);
    expect(looksLikeClaude(argv('bash'))).toBe(false);
    expect(looksLikeClaude(argv('node', 'server.js', '--name', 'claude'))).toBe(false);
    expect(looksLikeClaude(argv('vim', 'claude-notes.md'))).toBe(false);
    expect(looksLikeClaude('')).toBe(false);
  });
});

describe('parseProbe', () => {
  const answer = (claude: string, configDir = '') =>
    [
      'Welcome to a chatty .bashrc',
      '',
      '@@REFRAME@@',
      '/home/ann/.local/bin:/usr/bin:/mnt/c/Users/Ann/AppData/Roaming/npm',
      claude,
      '/home/ann',
      configDir,
      'MOUNT=/mnt/c/',
    ].join('\r\n');

  it('finds claude, the PATH, the home and where the drives are, past what the shell prints', () => {
    expect(parseProbe('Ubuntu', answer('/home/ann/.local/bin/claude'))).toEqual({
      distro: 'Ubuntu',
      claude: '/home/ann/.local/bin/claude',
      path: '/home/ann/.local/bin:/usr/bin:/mnt/c/Users/Ann/AppData/Roaming/npm',
      home: '/home/ann',
      claudeHome: '/home/ann/.claude',
      mountRoot: '/mnt/',
    });
    expect(parseProbe('Ubuntu', answer('/usr/bin/claude', '/data/claude'))?.claudeHome).toBe('/data/claude');
  });

  it('does not count the Windows claude seen through the PATH, nor a missing one', () => {
    expect(parseProbe('Ubuntu', answer('/mnt/c/Users/Ann/AppData/Roaming/npm/claude'))?.claude).toBeNull();
    expect(parseProbe('Ubuntu', answer(''))?.claude).toBeNull();
  });

  it('gives up without an answer', () => {
    expect(parseProbe('Ubuntu', 'command not found')).toBeNull();
  });
});

describe('what a terminal tab in WSL holds', () => {
  const argv = (...args: string[]) => args.join('\0');

  it('knows shells by their command lines, login shells too', () => {
    expect(isShellCommand(argv('-bash'))).toBe(true);
    expect(isShellCommand(argv('/usr/bin/zsh', '-l'))).toBe(true);
    expect(isShellCommand(argv('bash', '-lic', 'claude'))).toBe(true);
    expect(isShellCommand(argv('fish'))).toBe(true);
    expect(isShellCommand(argv('tmux', 'new'))).toBe(false);
    expect(isShellCommand(argv('/init'))).toBe(false);
    expect(isShellCommand('')).toBe(false);
  });

  it('reads the parent from /proc/<pid>/stat, whatever the program is called', () => {
    expect(parentOf('29024 (claude) S 341 29024 341 34816 29024 4194304')).toBe(341);
    expect(parentOf('77 (my (odd) prog) R 12 77 12 0')).toBe(12);
    expect(parentOf('')).toBeNull();
    expect(parentOf('1 (init) S 0 0')).toBeNull();
  });

  it('finds the interop socket in the environment', () => {
    expect(interopOf(argv('HOME=/home/ann', 'WSL_INTEROP=/run/WSL/340_interop', 'TERM=xterm'))).toBe(
      '/run/WSL/340_interop',
    );
    expect(interopOf(argv('HOME=/home/ann'))).toBeNull();
    expect(interopOf('')).toBeNull();
  });

  it('runs the Windows program as node through the socket, which the Windows side must see in WSLENV', () => {
    expect(parentPidCommand('/run/WSL/340_interop', '/mnt/c/reframe/reframe.exe')).toEqual([
      'env',
      'WSL_INTEROP=/run/WSL/340_interop',
      'ELECTRON_RUN_AS_NODE=1',
      'WSLENV=ELECTRON_RUN_AS_NODE',
      '/mnt/c/reframe/reframe.exe',
      '-e',
      'process.stdout.write(String(process.ppid))',
    ]);
  });
});

// The distributions' share is a Windows path (WSL is Windows only).
describe.runIf(process.platform === 'win32')('WslDistros: the shells around a claude', () => {
  let share = '';
  const before = process.env.REFRAME_WSL_SHARE;
  afterEach(() => {
    process.env.REFRAME_WSL_SHARE = before;
    if (share) fs.rmSync(share, { recursive: true, force: true });
  });

  /** A distribution "Dist" whose /proc has these processes: pid → [parent, command line, children]. */
  function distro(processes: Record<number, [number, string, number[]]>): WslDistros {
    share = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-wsl-'));
    for (const [pid, [parent, cmdline, children]] of Object.entries(processes)) {
      const folder = path.join(share, 'Dist', 'proc', pid);
      fs.mkdirSync(path.join(folder, 'task', pid), { recursive: true });
      fs.writeFileSync(path.join(folder, 'stat'), `${pid} (${cmdline.split('\0')[0]}) S ${parent} ${pid}`);
      fs.writeFileSync(path.join(folder, 'cmdline'), `${cmdline}\0`);
      fs.writeFileSync(path.join(folder, 'task', pid, 'children'), children.map((c) => `${c} `).join(''));
    }
    process.env.REFRAME_WSL_SHARE = share;
    return new WslDistros(() => {});
  }

  it("are the shells up to WSL's own process for the terminal, innermost first", async () => {
    const wsl = distro({
      10: [2, '/init', [20]],
      20: [10, '-bash', [21]],
      21: [20, 'bash', [30]],
      30: [21, '/usr/local/bin/claude', []],
    });
    expect(await wsl.shellsOf('Dist', 30)).toEqual([21, 20]);
    expect(await wsl.idle('Dist', 21)).toBe(false);
    expect(await wsl.idle('Dist', 30)).toBe(true);
    expect(await wsl.idle('Dist', 99)).toBe(false);
  });

  it("are none when claude is the terminal's own program, or anything else is in between", async () => {
    const wsl = distro({
      10: [2, '/init', [30, 15]],
      30: [10, '/usr/local/bin/claude', []],
      15: [10, 'tmux', [20]],
      20: [15, '-bash', [31]],
      31: [20, '/usr/local/bin/claude', []],
    });
    expect(await wsl.shellsOf('Dist', 30)).toEqual([]);
    expect(await wsl.shellsOf('Dist', 31)).toEqual([]);
    expect(await wsl.shellsOf('Dist', 99)).toEqual([]);
  });
});
