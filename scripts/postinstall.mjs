// npm unpacks node-pty's prebuilt spawn-helper (macOS) without its executable bit, and without it node-pty
// cannot start any process ("posix_spawnp failed"). Runs after every npm install / npm ci.
import fs from 'node:fs';
import path from 'node:path';

const prebuilds = path.join(import.meta.dirname, '..', 'node_modules', 'node-pty', 'prebuilds');
if (process.platform !== 'win32' && fs.existsSync(prebuilds)) {
  for (const entry of fs.readdirSync(prebuilds)) {
    const helper = path.join(prebuilds, entry, 'spawn-helper');
    if (fs.existsSync(helper)) fs.chmodSync(helper, 0o755);
  }
}
