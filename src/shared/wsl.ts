// Folders in WSL distributions as Windows sees them (\\wsl.localhost\Ubuntu\home\ann\shop) and as Linux does
// (/home/ann/shop). A session whose folder is one of these runs its claude in that distribution.

/** Where Windows shows the distributions; `\\wsl$` is the older name of the same. */
export const WSL_SHARES = ['\\\\wsl.localhost', '\\\\wsl$'];

export interface WslPath {
  distro: string;
  /** The absolute Linux path. */
  path: string;
}

/**
 * The distribution and Linux path of a folder in WSL, or null for any other folder. `share` is where the
 * distributions are (tests use a plain folder); the usual names are always recognized.
 */
export function parseWslPath(windowsPath: string, share?: string): WslPath | null {
  const normalized = windowsPath.replace(/\//g, '\\');
  for (const root of [...(share ? [share] : []), ...WSL_SHARES]) {
    const prefix = `${root.replace(/[\\/]+$/, '')}\\`;
    if (normalized.slice(0, prefix.length).toLowerCase() !== prefix.toLowerCase()) continue;
    const [distro, ...rest] = normalized.slice(prefix.length).split('\\').filter(Boolean);
    if (!distro) return null;
    return { distro, path: `/${rest.join('/')}` };
  }
  return null;
}

/** The Windows path of a Linux path in a distribution, under `share`. */
export function wslWindowsPath(distro: string, linuxPath: string, share: string): string {
  const rest = linuxPath.split('/').filter(Boolean);
  return [share.replace(/[\\/]+$/, ''), distro, ...rest].join('\\');
}

/**
 * A Windows path as programs in `distro` reach it: a drive under the mount root (C:\x → /mnt/c/x), a folder of
 * the distribution itself as its Linux path. Null for what the distribution cannot reach (another one's folders).
 */
export function linuxPathOf(windowsPath: string, distro: string, mountRoot: string, share?: string): string | null {
  const drive = /^([A-Za-z]):(?:[\\/](.*))?$/.exec(windowsPath);
  if (drive) {
    const rest = (drive[2] ?? '').split(/[\\/]/).filter(Boolean);
    return `${mountRoot.replace(/\/+$/, '')}/${drive[1]!.toLowerCase()}${rest.map((part) => `/${part}`).join('')}`;
  }
  const own = parseWslPath(windowsPath, share);
  return own && own.distro.toLowerCase() === distro.toLowerCase() ? own.path : null;
}
