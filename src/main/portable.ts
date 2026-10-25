// The portable Windows copy (the ZIP): a folder "data" next to reframe.exe holds everything reframe keeps, so the
// whole folder can move (a USB stick, say) and it runs apart from an installed reframe. An installed copy has no
// such folder next to it.
import fs from 'node:fs';
import path from 'node:path';

/** The data folder of a portable copy, or null when this is no portable copy. */
export function portableDataDir(execPath: string, platform: NodeJS.Platform = process.platform): string | null {
  if (platform !== 'win32') return null;
  const dir = path.win32.join(path.win32.dirname(execPath), 'data');
  try {
    return fs.statSync(dir).isDirectory() ? dir : null;
  } catch {
    return null;
  }
}
