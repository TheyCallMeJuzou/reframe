// Builds the website (site/, published on GitHub Pages by .github/workflows/pages.yml) into a folder: the pages, the
// pictures they use from docs/ (kept there once, for the README too), and the download buttons pointed at the files
// of the newest release, which it asks GitHub for (with GH_TOKEN, if set). A button whose file that release does not
// have keeps its link to the release page.
// node scripts/site.mjs [output folder, default _site]
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const root = path.join(import.meta.dirname, '..');
const REPO = 'TheyCallMeJuzou/reframe';
/** The pictures the pages use, from docs/, served under img/. */
const IMAGES = [
  'logo/reframe-icon.svg',
  'screenshots/hero.png',
  'screenshots/drag-tile.gif',
  'screenshots/windows.png',
  'screenshots/files.gif',
  'screenshots/social-preview.png',
];

/** The newest published release (drafts and pre-releases do not count, as for the updates). */
export async function latestRelease(token = process.env.GH_TOKEN) {
  const response = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: { Accept: 'application/vnd.github+json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
  });
  if (!response.ok) throw new Error(`GitHub answered ${response.status} for the latest release`);
  return response.json();
}

/**
 * Fills a page in with `release`: the version where an element has `data-version`, the release page where a link has
 * `data-release`, and each file's download link where a link has `data-asset="reframe-Setup-{version}.exe"` (its
 * `href` comes right before). Returns the page and the files the release does not have.
 */
export function fillRelease(html, release) {
  const version = release.tag_name.replace(/^v/, '');
  const files = new Map(release.assets.map((asset) => [asset.name, asset.browser_download_url]));
  const missing = [];
  const page = html
    .replace(/(<(\w+)\b[^>]*\sdata-version[^>]*>)[^<]*(<\/\2>)/g, (_all, open, _tag, close) => open + version + close)
    .replace(/href="[^"]*"(\s+data-release\b)/g, (_all, rest) => `href="${release.html_url}"${rest}`)
    .replace(/href="([^"]*)"(\s+data-asset="([^"]+)")/g, (all, fallback, rest, pattern) => {
      const name = pattern.replaceAll('{version}', version);
      const url = files.get(name);
      if (!url) missing.push(name);
      return `href="${url ?? fallback}"${rest}`;
    });
  return { page, missing };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const out = path.resolve(process.argv[2] ?? path.join(root, '_site'));
  const release = await latestRelease();
  fs.rmSync(out, { recursive: true, force: true });
  fs.cpSync(path.join(root, 'site'), out, { recursive: true });
  for (const image of IMAGES) {
    fs.cpSync(path.join(root, 'docs', image), path.join(out, 'img', path.basename(image)));
  }
  for (const name of fs.readdirSync(out).filter((file) => file.endsWith('.html'))) {
    const file = path.join(out, name);
    const { page, missing } = fillRelease(fs.readFileSync(file, 'utf8'), release);
    for (const asset of missing) console.warn(`${name}: ${release.tag_name} has no ${asset}, linking the release page`);
    fs.writeFileSync(file, page);
  }
  console.log(`${path.relative(root, out) || out}: the website, with the downloads of ${release.tag_name}`);
}
