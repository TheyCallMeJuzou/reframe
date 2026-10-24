// Lists of copied files in the clipboard formats of macOS and Linux file managers.
import { fileURLToPath } from 'node:url';

/** The strings in a property list (macOS file names). */
export function plistStrings(xml: string): string[] {
  return [...xml.matchAll(/<string>([^<]*)<\/string>/g)].map((m) => decodeXml(m[1]!));
}

/** Local paths from a list of file:// URLs, one per line (a GNOME list starts with "copy" or "cut"). */
export function fileUrls(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith('file://'))
    .map((url) => fileURLToPath(url));
}

const decodeXml = (value: string) =>
  value
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, '&');
