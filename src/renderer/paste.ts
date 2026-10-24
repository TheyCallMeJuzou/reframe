// What reframe types into a session for files and images, the way a terminal would.

/** File paths as one would type them: quoted where needed, separated by spaces, with a space after. */
export function formatPaths(paths: string[], platform: string): string {
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
