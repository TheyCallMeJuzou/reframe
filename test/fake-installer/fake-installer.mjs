// Stand-in for reframe's installer in the end-to-end tests of the updater: reframe starts it (through
// node, as REFRAME_FAKE_INSTALLER says) with the downloaded installer and the arguments the real one would
// get. It writes them, and the installer's size, to REFRAME_FAKE_INSTALLER_LOG, and installs nothing.
import fs from 'node:fs';

const [installer, ...args] = process.argv.slice(2);
fs.appendFileSync(
  process.env.REFRAME_FAKE_INSTALLER_LOG,
  `${JSON.stringify({ installer, args, bytes: fs.readFileSync(installer).length })}\n`,
);
