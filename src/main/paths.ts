// Folders as people read them.

/** A folder under the home folder as "~" plus the rest (e.g. "~\projects\shop"), any other as it is. */
export function homeRelative(folder: string, home: string, windows: boolean): string {
  const base = home.replace(/[\\/]+$/, '');
  if (!base) return folder;
  const same = (a: string, b: string) => (windows ? a.toLowerCase() === b.toLowerCase() : a === b);
  if (same(folder, base)) return '~';
  const next = folder.charAt(base.length);
  const isSeparator = next === '/' || (windows && next === '\\');
  return isSeparator && same(folder.slice(0, base.length), base) ? `~${folder.slice(base.length)}` : folder;
}
