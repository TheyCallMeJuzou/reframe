// Finds the claude executable the same way the user's shell would.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export interface ClaudeBinary {
  file: string;
  /** Arguments placed before claude's own (e.g. the script path when running a .js override). */
  prefixArgs: string[];
  /** Extra environment needed to run `file` (e.g. ELECTRON_RUN_AS_NODE for a .js override). */
  env: Record<string, string>;
  /** Where it was found, for diagnostics. */
  source: string;
}

export interface ResolveOptions {
  env: NodeJS.ProcessEnv;
  platform: NodeJS.Platform;
  /** Explicit path from settings or REFRAME_CLAUDE_BIN. */
  override?: string;
  /**
   * Runs .js overrides. Defaults to Electron as node, which works on macOS/Linux; on Windows electron.exe
   * is a GUI program that does not get the pseudo console's stdio, so tests pass a real node there.
   */
  nodeRunner?: { file: string; env: Record<string, string> };
  isFile?: (p: string) => boolean;
  readFile?: (p: string) => string;
}

const defaultIsFile = (p: string): boolean => {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
};

export function resolveClaude(options: ResolveOptions): ClaudeBinary | null {
  const isFile = options.isFile ?? defaultIsFile;
  const readFile = options.readFile ?? ((p: string) => fs.readFileSync(p, 'utf8'));
  const { platform, env } = options;

  if (options.override) {
    if (/\.(c|m)?js$/i.test(options.override)) {
      const runner = options.nodeRunner ?? { file: process.execPath, env: { ELECTRON_RUN_AS_NODE: '1' } };
      return { file: runner.file, prefixArgs: [options.override], env: runner.env, source: options.override };
    }
    return wrap(options.override, platform, readFile);
  }

  const pathKey = Object.keys(env).find((k) => k.toUpperCase() === 'PATH') ?? 'PATH';
  const dirs = (env[pathKey] ?? '')
    .split(platform === 'win32' ? ';' : ':')
    .map((d) => d.trim().replace(/^"(.*)"$/, '$1'))
    .filter(Boolean);
  const join = platform === 'win32' ? path.win32.join : path.posix.join;

  if (platform === 'win32') {
    const exts = (env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD').split(';').filter(Boolean);
    for (const dir of dirs) {
      for (const ext of exts) {
        const candidate = join(dir, `claude${ext.toLowerCase()}`);
        if (isFile(candidate)) return wrap(candidate, platform, readFile);
      }
    }
    return null;
  }

  // GUI apps on macOS/Linux often start with a minimal PATH, so also look where installers put claude.
  const home = os.homedir();
  const fallbacks = [
    join(home, '.local', 'bin'),
    join(home, '.claude', 'local'),
    join(home, '.npm-global', 'bin'),
    '/opt/homebrew/bin',
    '/usr/local/bin',
  ];
  for (const dir of [...dirs, ...fallbacks]) {
    const candidate = join(dir, 'claude');
    if (isFile(candidate)) return wrap(candidate, platform, readFile);
  }
  return null;
}

function wrap(file: string, platform: NodeJS.Platform, readFile: (p: string) => string): ClaudeBinary {
  if (platform === 'win32' && /\.(cmd|bat)$/i.test(file)) {
    // npm's cmd-shim ends with `"%dp0%\...\claude.exe" %*`; running that exe directly makes the
    // pty's pid the claude pid, which is what Claude Code's session registry is keyed by.
    try {
      const match = readFile(file).match(/"%dp0%\\([^"]+\.exe)"/i);
      if (match?.[1]) {
        return { file: path.win32.join(path.win32.dirname(file), match[1]), prefixArgs: [], env: {}, source: file };
      }
    } catch {
      // fall through to cmd.exe
    }
    return { file: 'cmd.exe', prefixArgs: ['/d', '/s', '/c', file], env: {}, source: file };
  }
  return { file, prefixArgs: [], env: {}, source: file };
}
