// What reframe types into a session for files and images, the way a terminal would.
import { linuxPathOf } from '@shared/wsl';

/**
 * File paths as one would type them: quoted where needed, separated by spaces, with a space after. For a session
 * in WSL they are its Linux paths (C:\x → /mnt/c/x), quoted for its shell.
 */
export function formatPaths(paths: string[], platform: string, wsl?: { distro: string; mountRoot: string }): string {
  if (wsl) {
    const linux = paths.map((p) => linuxPathOf(p, wsl.distro, wsl.mountRoot) ?? p);
    return formatPaths(linux, 'linux');
  }
  const quote =
    platform === 'win32'
      ? (p: string) => (/[\s&()^'"`,;=!%]/.test(p) ? `"${p}"` : p)
      : (p: string) => (/^[\w@%+=:,./-]+$/.test(p) ? p : `'${p.replace(/'/g, `'\\''`)}'`);
  return paths.length > 0 ? `${paths.map(quote).join(' ')} ` : '';
}

/** The key Claude Code takes to paste the clipboard's image: Alt+V on Windows, Ctrl+V elsewhere. */
export function imagePasteKey(platform: string): string {
  return platform === 'win32' ? '\x1bv' : '\x16';
}

/**
 * Whether reframe saves the clipboard's image to a file and types its path, instead of sending Claude Code its
 * image-paste key. In WSL Claude Code cannot reach the clipboard at all. On Windows it can, but reads it through
 * PowerShell, which took about ten seconds for a small picture; a path it attaches at once.
 */
export function pastesImageAsFile(platform: string, wsl: boolean): boolean {
  return wsl || platform === 'win32';
}
