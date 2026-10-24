// What is on the system clipboard, as far as pasting into a session goes: text, files copied in a file manager
// (typed in as their paths), or an image (Claude Code reads that from the clipboard itself).
import { clipboard } from 'electron';
import type { ClipboardContents } from '@shared/ipc';
import { fileUrls, plistStrings } from './clipboard-formats';
import { win32 } from './win32';

export async function readClipboard(): Promise<ClipboardContents> {
  const [text, image, files] = await Promise.all([
    clipboard.readText().catch(() => ''),
    clipboard.has('image/png').catch(() => false),
    copiedFiles(),
  ]);
  return { text, image, files };
}

async function copiedFiles(): Promise<string[]> {
  try {
    if (process.platform === 'win32') return win32()?.clipboardFiles() ?? [];
    if (process.platform === 'darwin') {
      const list = plistStrings(await textOf(osFormat('NSFilenamesPboardType')));
      return list.length > 0 ? list : fileUrls(await textOf(osFormat('public.file-url')));
    }
    return fileUrls((await textOf('x-special/gnome-copied-files')) || (await textOf('text/uri-list')));
  } catch {
    return [];
  }
}

/** A platform clipboard format by its own name (Electron's way to reach formats without a MIME type). */
const osFormat = (format: string) => `electron application/osclipboard;format="${format}"`;

/** The clipboard's data of `type` as text, or "" if there is none. */
async function textOf(type: string): Promise<string> {
  if (!(await clipboard.has(type))) return '';
  for (const item of await clipboard.read()) {
    if (item.types.includes(type)) return ((await item.getType(type)) as Blob).text();
  }
  return '';
}
