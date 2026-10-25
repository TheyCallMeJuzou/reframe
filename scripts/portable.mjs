// Makes the portable Windows copy out of the unpacked app (npm run dist:dir, or the release build):
// dist/reframe-<version>-win-portable.zip, with a folder "data" next to reframe.exe, where that copy then keeps all
// its data (see src/main/portable.ts). Windows only: it zips with the tar that comes with Windows.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..');
const { version } = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const dist = path.join(root, 'dist');
const unpacked = path.join(dist, 'win-unpacked');
const name = `reframe-${version}-win-portable`;
const folder = path.join(dist, name);
const zip = path.join(dist, `${name}.zip`);

if (!fs.existsSync(path.join(unpacked, 'reframe.exe'))) {
  console.error(`${path.relative(root, unpacked)} has no reframe.exe: build it first (npm run dist:dir).`);
  process.exit(1);
}
fs.rmSync(folder, { recursive: true, force: true });
fs.rmSync(zip, { force: true });
fs.cpSync(unpacked, folder, { recursive: true });
// Where to look for new versions. electron-builder writes it only when building the installer (as the release
// does); after `npm run dist:dir` it is missing.
const updateConfig = path.join(folder, 'resources', 'app-update.yml');
if (!fs.existsSync(updateConfig)) {
  fs.writeFileSync(
    updateConfig,
    'owner: TheyCallMeJuzou\nrepo: reframe\nprovider: github\nreleaseType: draft\nupdaterCacheDirName: reframe-updater\n',
  );
}
fs.mkdirSync(path.join(folder, 'data'));
fs.writeFileSync(
  path.join(folder, 'data', 'README.txt'),
  [
    'reframe keeps everything here: its sessions, windows, settings and log.',
    'Keep this folder next to reframe.exe. Without it, reframe keeps its data where an installed one does',
    '(%APPDATA%\\reframe).',
    '',
  ].join('\r\n'),
);
// Not just "tar": Git's GNU tar can come first on the PATH, and it does not make zip files.
const tar = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe');
execFileSync(tar, ['-c', '-f', zip, '--format', 'zip', name], { cwd: dist, stdio: 'inherit' });
console.log(`${path.relative(root, zip)}: ${Math.round(fs.statSync(zip).size / 1048576)} MB`);
