// WSL distributions (Windows only) and what it takes to run claude in one. That is asked once per distribution and
// run of reframe, through the user's login shell (where nvm and the like put claude and node on the PATH), which
// also starts the distribution. wsl.exe and the share can be replaced for tests (REFRAME_WSL_EXE, REFRAME_WSL_SHARE).
import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { EventEmitter } from 'node:events';
import fs from 'node:fs';
import path from 'node:path';
import { WSL_SHARES } from '@shared/wsl';
import type { LogFn } from '../log';

export interface WslInfo {
  distro: string;
  /** claude in the distribution itself; null when there is none (one on a Windows drive does not count). */
  claude: string | null;
  /** PATH of the user's login shell, which claude (and node, for an npm install of it) needs. */
  path: string;
  home: string;
  /** Claude Code's config folder, as a Linux path. */
  claudeHome: string;
  /** Where the Windows drives are, e.g. /mnt/. */
  mountRoot: string;
}

export interface WslCommand {
  file: string;
  /** Arguments before wsl.exe's own (the script of a stand-in). */
  prefixArgs: string[];
  env: Record<string, string>;
}

/** wsl.exe, or a stand-in script run by REFRAME_NODE (as in the end-to-end tests). */
export function wslCommand(env: NodeJS.ProcessEnv = process.env): WslCommand {
  const override = env.REFRAME_WSL_EXE;
  if (override && /\.(c|m)?js$/i.test(override)) {
    return env.REFRAME_NODE
      ? { file: env.REFRAME_NODE, prefixArgs: [override], env: {} }
      : { file: process.execPath, prefixArgs: [override], env: { ELECTRON_RUN_AS_NODE: '1' } };
  }
  return {
    file: override || path.win32.join(env.SystemRoot ?? 'C:\\Windows', 'System32', 'wsl.exe'),
    prefixArgs: [],
    env: {},
  };
}

/** Where Windows shows the distributions' files. */
export const wslShare = (env: NodeJS.ProcessEnv = process.env): string => env.REFRAME_WSL_SHARE || WSL_SHARES[0]!;

const MARKER = '@@REFRAME@@';

/**
 * Asks the user's login shell (interactive, so .bashrc runs too) for what reframe needs, between markers so that
 * whatever the shell's startup files print does not get in the way; falls back to plain sh when that shell fails.
 */
export const PROBE_SCRIPT = [
  's="${SHELL:-/bin/sh}"',
  `ask='printf "\\n${MARKER}\\n%s\\n%s\\n%s\\n%s\\n" "$PATH" "$(command -v claude)" "$HOME" "\${CLAUDE_CONFIG_DIR:-}"'`,
  'out=$("$s" -lic "$ask" 2>/dev/null </dev/null)',
  `case "$out" in *${MARKER}*) ;; *) out=$(sh -c "$ask") ;; esac`,
  `printf '%s\\n' "$out" | sed -n '/^${MARKER}$/,$p'`,
  `printf 'MOUNT=%s\\n' "$(wslpath -u 'C:\\' 2>/dev/null)"`,
].join('\n');

/** wsl.exe writes UTF-16 unless WSL_UTF8 is set, which older versions ignore. */
export function decodeWslOutput(output: Buffer): string {
  const utf16 = output.length >= 2 && output[1] === 0 && output[0] !== 0;
  return (utf16 ? output.toString('utf16le') : output.toString('utf8')).replace(/^\uFEFF/, '');
}

/** Distribution names from `wsl.exe -l -q`. */
export function parseDistroList(output: string): string[] {
  return output
    .split(/\r?\n/)
    .map((line) => line.replace(/\0/g, '').trim())
    .filter(Boolean);
}

/** What the probe found, or null when it did not answer. */
export function parseProbe(distro: string, output: string): WslInfo | null {
  const lines = output.replace(/\r/g, '').split('\n');
  const at = lines.indexOf(MARKER);
  if (at < 0) return null;
  const [pathValue = '', claude = '', home = '', configDir = ''] = lines.slice(at + 1, at + 5);
  if (!home.startsWith('/')) return null;
  const mount = lines.find((line) => line.startsWith('MOUNT='))?.slice('MOUNT='.length) || '/mnt/c/';
  const mountRoot = mount.replace(/[^/]+\/*$/, '') || '/mnt/';
  // The Windows PATH is part of WSL's; a claude found there is the Windows one, which cannot run here.
  const inDistro = claude.startsWith('/') && !claude.startsWith(mountRoot);
  return {
    distro,
    claude: inDistro ? claude : null,
    path: pathValue,
    home,
    claudeHome: configDir.startsWith('/') ? configDir : `${home.replace(/\/+$/, '')}/.claude`,
    mountRoot,
  };
}

/**
 * Whether a Linux command line (argv, NUL-separated, as /proc/<pid>/cmdline has it) is a Claude Code's: the program
 * `claude` (the native install, or a link to it), or node running its script (an npm, pnpm or yarn install).
 */
export function looksLikeClaude(cmdline: string): boolean {
  return cmdline
    .split('\0')
    .slice(0, 2)
    .some((arg) => /(^|\/)claude$|\/claude-code\/|\/\.local\/share\/claude\//.test(arg));
}

type Events = {
  /** Claude Code was found in this distribution. */
  found: [distro: string];
};

/**
 * How long reading from a distribution's share (its Claude Code folder, its /proc) may take before it counts as no
 * answer: the share starts a distribution that is not running, which takes seconds.
 */
export const SHARE_TIMEOUT_MS = 3_000;

/** How long a distribution without claude is taken at its word before it is asked again. */
const NO_CLAUDE_MS = 30_000;
/** How long the list of running distributions holds (the session list asks every few seconds). */
const RUNNING_MS = 3_000;

export class WslDistros extends EventEmitter<Events> {
  readonly command = wslCommand();
  readonly share = wslShare();
  private infos = new Map<string, Promise<WslInfo | null>>();
  private known = new Map<string, WslInfo>();
  private runningNow: { at: number; names: Promise<string[]> } | null = null;
  /** Processes that keep distributions running (started from the session list). */
  private keepers = new Map<string, ChildProcess>();

  constructor(private log: LogFn) {
    super();
  }

  /** What it takes to run claude in `distro`; starts the distribution. Asked again a while later when it has none. */
  info(distro: string): Promise<WslInfo | null> {
    const key = distro.toLowerCase();
    let pending = this.infos.get(key);
    if (!pending) {
      pending = this.probe(distro);
      this.infos.set(key, pending);
      void pending.then((info) => {
        if (info?.claude) {
          this.known.set(key, info);
          this.emit('found', distro);
        } else {
          setTimeout(() => this.infos.delete(key), NO_CLAUDE_MS).unref();
        }
      });
    }
    return pending;
  }

  /** What is known already about `distro`, without asking it. */
  cached(distro: string): WslInfo | null {
    return this.known.get(distro.toLowerCase()) ?? null;
  }

  /** The distributions there are (none without WSL). */
  async list(): Promise<string[]> {
    return parseDistroList(await this.run(['-l', '-q'], 10_000).catch(() => ''));
  }

  /**
   * The distributions that run now, without Docker Desktop's own (asking does not start any); an answer up to
   * `maxAgeMs` old will do.
   */
  running(maxAgeMs = RUNNING_MS): Promise<string[]> {
    if (!this.runningNow || Date.now() - this.runningNow.at > maxAgeMs) {
      const names = this.run(['-l', '--running', '-q'], 10_000)
        .then(parseDistroList, () => [])
        .then((list) => list.filter((name) => !/^docker-desktop/i.test(name)));
      this.runningNow = { at: Date.now(), names };
    }
    return this.runningNow.names;
  }

  /** Whether a Linux process runs in `distro` (as its /proc shows it). */
  alive(distro: string, pid: number): Promise<boolean> {
    return fs.promises.access(path.win32.join(this.share, distro, 'proc', String(pid))).then(
      () => true,
      () => false,
    );
  }

  /**
   * Whether the Linux process `pid` runs in `distro` and is a claude. A claude that was killed leaves its registry
   * entry behind, and a distribution numbers its processes anew every time it starts: the pid in such an entry is
   * soon another program's, which must be neither listed as a claude nor ended as one.
   */
  async isClaude(distro: string, pid: number): Promise<boolean> {
    const cmdline = await fs.promises
      .readFile(path.win32.join(this.share, distro, 'proc', String(pid), 'cmdline'), 'utf8')
      .catch(() => '');
    return looksLikeClaude(cmdline);
  }

  /**
   * Ends a claude running in `distro`, from outside (taking it over): an interrupt as from Ctrl+C, which Claude
   * Code answers by ending cleanly, a second one, then a plain termination.
   */
  async stopClaude(distro: string, pid: number): Promise<'stopped' | 'failed'> {
    // Never a program that merely has the pid of a claude by now.
    if (!(await this.isClaude(distro, pid))) {
      this.log('warn', 'not a claude (any more), left alone', { distro, pid });
      return 'failed';
    }
    for (const signal of ['INT', 'INT', 'TERM']) {
      await this.run(['-d', distro, '-e', 'kill', '-s', signal, String(pid)], 10_000).catch(() => '');
      for (let waited = 0; waited < 3000; waited += 200) {
        if (!(await this.alive(distro, pid))) return 'stopped';
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
    }
    return (await this.alive(distro, pid)) ? 'failed' : 'stopped';
  }

  /** Starts `distro` and keeps it running until release() (WSL stops a distribution soon when nothing runs in it). */
  keep(distro: string): void {
    if (this.keepers.has(distro)) return;
    const { file, prefixArgs, env } = this.command;
    const keeper = spawn(file, [...prefixArgs, '-d', distro, '-e', 'sleep', 'infinity'], {
      env: { ...process.env, ...env },
      stdio: 'ignore',
      windowsHide: true,
    });
    keeper.on('exit', () => this.keepers.delete(distro));
    this.keepers.set(distro, keeper);
    this.runningNow = null;
    this.log('info', 'wsl distribution started from the session list', { distro });
  }

  /** Lets go of the distributions kept running (reframe quits). */
  release(): void {
    for (const keeper of this.keepers.values()) keeper.kill();
    this.keepers.clear();
  }

  private async probe(distro: string): Promise<WslInfo | null> {
    const started = Date.now();
    try {
      const info = parseProbe(distro, await this.run(['-d', distro, '-e', 'sh', '-c', PROBE_SCRIPT], 60_000));
      this.log(info ? 'info' : 'warn', 'wsl distribution asked', {
        distro,
        ms: Date.now() - started,
        claude: info?.claude ?? 'not found',
        ...(info ? {} : { problem: 'no answer' }),
      });
      return info;
    } catch (error) {
      this.log('warn', 'wsl distribution could not be asked', { distro, error: String(error) });
      return null;
    }
  }

  private run(args: string[], timeout: number): Promise<string> {
    const { file, prefixArgs, env } = this.command;
    return new Promise((resolve, reject) => {
      execFile(
        file,
        [...prefixArgs, ...args],
        { env: { ...process.env, ...env, WSL_UTF8: '1' }, encoding: 'buffer', timeout, windowsHide: true },
        (error, stdout) => (error ? reject(error) : resolve(decodeWslOutput(stdout))),
      );
    });
  }
}
