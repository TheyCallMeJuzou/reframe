// After SignPath has signed the Windows release (see .github/workflows/release.yml): checks the signatures of the
// installer and of reframe.exe in the portable ZIP, puts both in place of the unsigned files in dist/, and brings
// the installer's block map and latest.yml (its size and checksum, which installed copies check an update
// against) up to date. Windows only.
// node scripts/signed.mjs [folder with the signed files, default dist/signed]
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const root = path.join(import.meta.dirname, '..');
const require = createRequire(import.meta.url);
// electron-builder's own block map, so differential updates keep working (app-builder-lib comes with it).
const { buildBlockMap } = require('app-builder-lib/out/targets/blockmap/blockmap.js');
const tar = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe');

/** The Authenticode status of a file ("Valid", "NotSigned", …) and who signed it. */
export function signature(file) {
  const out = execFileSync(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      '$s = Get-AuthenticodeSignature -LiteralPath $env:SIGNED_FILE; "$($s.Status)|$($s.SignerCertificate.Subject)"',
    ],
    { env: { ...process.env, SIGNED_FILE: file }, encoding: 'utf8' },
  ).trim();
  const [status, subject] = out.split('|');
  return { status, subject };
}

function mustBeSigned(file, label) {
  const { status, subject } = signature(file);
  if (status !== 'Valid') throw new Error(`${label} is not signed (${status})`);
  console.log(`${label}: signed by ${subject}`);
}

/** Fails unless the installer and the portable reframe.exe in `folder` carry a valid signature. */
export function checkSigned(folder, version) {
  mustBeSigned(path.join(folder, `reframe-Setup-${version}.exe`), `reframe-Setup-${version}.exe`);
  const portable = `reframe-${version}-win-portable`;
  const unpacked = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-signed-'));
  try {
    execFileSync(tar, ['-x', '-f', path.join(folder, `${portable}.zip`), '-C', unpacked, `${portable}/reframe.exe`]);
    mustBeSigned(path.join(unpacked, portable, 'reframe.exe'), `${portable}.zip: reframe.exe`);
  } finally {
    fs.rmSync(unpacked, { recursive: true, force: true });
  }
}

/** Copies the signed files from `folder` into `dist`, then renews the installer's block map and latest.yml. */
export async function useSigned(folder, dist, version) {
  const installer = `reframe-Setup-${version}.exe`;
  const latest = path.join(dist, 'latest.yml');
  const before = fs.readFileSync(latest, 'utf8');
  const old = { sha512: /^sha512: (.+)$/m.exec(before)?.[1], size: /^\s+size: (\d+)$/m.exec(before)?.[1] };
  if (!old.sha512 || !old.size || !before.includes(`url: ${installer}`)) throw new Error(`unexpected ${latest}`);

  fs.copyFileSync(path.join(folder, installer), path.join(dist, installer));
  const portable = `reframe-${version}-win-portable.zip`;
  fs.copyFileSync(path.join(folder, portable), path.join(dist, portable));
  const info = await buildBlockMap(path.join(dist, installer), 'gzip', path.join(dist, `${installer}.blockmap`));
  const after = before
    .replaceAll(`sha512: ${old.sha512}`, `sha512: ${info.sha512}`)
    .replace(`size: ${old.size}`, `size: ${info.size}`);
  fs.writeFileSync(latest, after);
  console.log(`latest.yml and ${installer}.blockmap: ${info.size} bytes, sha512 ${info.sha512}`);
  return info;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { version } = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const dist = path.join(root, 'dist');
  const folder = path.resolve(process.argv[2] ?? path.join(dist, 'signed'));
  checkSigned(folder, version);
  await useSigned(folder, dist, version);
}
