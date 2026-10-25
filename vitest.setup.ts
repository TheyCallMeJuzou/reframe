// Electron downloads its binary the first time it is loaded (since Electron 44 it has no install script). Test
// files that load it (through electron-updater, say) run in parallel workers, and on a fresh checkout (CI) each
// would start that download, tripping over the others. Loading it once here, before them, downloads it once.
import { createRequire } from 'node:module';

export default function setup(): void {
  createRequire(import.meta.url)('electron');
}
