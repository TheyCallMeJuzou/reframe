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
  /**
   * Windows: a launcher (batch file) reframe cannot see through, run by cmd.exe with a command line of its own (see
   * cmdCommandLine) instead of `prefixArgs`.
   */
  viaCmd?: string;
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
    return wrap(options.override, platform, readFile, isFile, () => null);
  }

  const pathKey = Object.keys(env).find((k) => k.toUpperCase() === 'PATH') ?? 'PATH';
  const dirs = (env[pathKey] ?? '')
    .split(platform === 'win32' ? ';' : ':')
    .map((d) => d.trim().replace(/^"(.*)"$/, '$1'))
    .filter(Boolean);
  const join = platform === 'win32' ? path.win32.join : path.posix.join;

  if (platform === 'win32') {
    const exts = (env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD').split(';').filter(Boolean);
    const onPath = (name: string, only?: string[]) => {
      for (const dir of dirs) {
        for (const ext of only ?? exts) {
          const candidate = join(dir, `${name}${ext.toLowerCase()}`);
          if (isFile(candidate)) return candidate;
        }
      }
      return null;
    };
    const claude = onPath('claude');
    return claude ? wrap(claude, platform, readFile, isFile, () => onPath('node', ['.exe'])) : null;
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
    if (isFile(candidate)) return wrap(candidate, platform, readFile, isFile, () => null);
  }
  return null;
}

/**
 * What runs a claude found on the PATH. On Windows that is often a package manager's batch file (npm, pnpm, yarn)
 * that starts the real program: reframe starts that program itself, so that the pty's process is claude, which is
 * what Claude Code's session registry is keyed by.
 */
function wrap(
  file: string,
  platform: NodeJS.Platform,
  readFile: (p: string) => string,
  isFile: (p: string) => boolean,
  findNode: () => string | null,
): ClaudeBinary {
  if (platform === 'win32' && /\.(cmd|bat)$/i.test(file)) {
    let text = '';
    try {
      text = readFile(file);
    } catch {
      // run as it is, below
    }
    const dir = path.win32.dirname(file);
    // The batch file's folder is %dp0% (npm, yarn) or %~dp0 (pnpm).
    const targets = [...text.matchAll(/"%~?dp0%?\\([^"]+)"/gi)].map((m) => m[1]!);
    // A script, run by the node next to the batch file or else the one on the PATH. (Its batch file names node.exe
    // too, so that is no program to take.)
    const script = targets.find((t) => /\.(c|m)?js$/i.test(t));
    const node = script && (isFile(path.win32.join(dir, 'node.exe')) ? path.win32.join(dir, 'node.exe') : findNode());
    if (script && node) {
      return { file: node, prefixArgs: [path.win32.join(dir, script)], env: {}, source: file };
    }
    // A program (npm's batch file for Claude Code's own claude.exe).
    const program = targets.find((t) => /\.exe$/i.test(t) && !/(^|\\)node\.exe$/i.test(t));
    if (program) return { file: path.win32.join(dir, program), prefixArgs: [], env: {}, source: file };
    return { file: 'cmd.exe', prefixArgs: [], env: {}, source: file, viaCmd: file };
  }
  return { file, prefixArgs: [], env: {}, source: file };
}

/**
 * The arguments for cmd.exe that run `script` (a batch file) with `args`, as one command line. With /s, cmd takes
 * off the quotes around the whole of it, so a path with spaces stays one path (cmd mangles it otherwise). Arguments
 * are quoted where needed; a quote inside one cannot get through cmd intact and is left out.
 */
export function cmdCommandLine(script: string, args: string[]): string {
  const quote = (arg: string) => (/^[\w\-.:\\/=@+,]+$/.test(arg) ? arg : `"${arg.replace(/"/g, '')}"`);
  return `/d /s /c "${[script, ...args].map(quote).join(' ')}"`;
}
