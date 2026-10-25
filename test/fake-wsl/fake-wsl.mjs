// Stand-in for wsl.exe in end-to-end tests. A distribution is a folder under REFRAME_WSL_SHARE (its root), with
// a user "ann"; its claude is the fake claude, run in the folder a Linux path names, with that distribution's
// ~/.claude. A distribution with a file "no-claude" in its root has none. Understands what reframe uses:
//   -l -q                                                  the distributions
//   -d <name> -e sh -c <script>                            the probe (answered with what reframe asks for)
//   -d <name> --cd <dir> --exec env PATH=… <claude> <args>   claude
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const share = process.env.REFRAME_WSL_SHARE;
const HOME = '/home/ann';
const option = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const windowsPath = (distro, linuxPath) => path.join(share, distro, ...linuxPath.split('/').filter(Boolean));
const distro = option('-d');
const exec = args.indexOf('--exec');

if (args[0] === '-l') {
  const distros = fs.readdirSync(share).filter((name) => fs.statSync(path.join(share, name)).isDirectory());
  process.stdout.write(distros.map((name) => `${name}\r\n`).join(''));
} else if (!distro || !fs.existsSync(path.join(share, distro))) {
  process.stderr.write('There is no distribution with the supplied name.\r\n');
  process.exitCode = 1;
} else if (option('-e') === 'sh') {
  const claude = fs.existsSync(path.join(share, distro, 'no-claude')) ? '' : '/usr/local/bin/claude';
  const answer = ['what a .bashrc prints', '@@REFRAME@@', '/usr/local/bin:/usr/bin', claude, HOME, '', 'MOUNT=/mnt/c/'];
  // A distribution takes a moment to start: reframe's windows are up before its sessions are.
  setTimeout(() => process.stdout.write(`${answer.join('\n')}\n`), 1500);
} else if (option('--cd') && exec >= 0) {
  // --exec env PATH=… <claude> <claude's own arguments>
  const claude = path.join(import.meta.dirname, '..', 'fake-claude', 'fake-claude.mjs');
  const child = spawn(process.execPath, [claude, ...args.slice(exec + 4)], {
    cwd: windowsPath(distro, option('--cd')),
    stdio: 'inherit',
    env: {
      ...process.env,
      REFRAME_CLAUDE_HOME: windowsPath(distro, `${HOME}/.claude`),
      REFRAME_FAKE_CWD: option('--cd'),
    },
  });
  child.on('exit', (code) => process.exit(code ?? 1));
} else {
  process.stderr.write(`fake wsl.exe does not know: ${args.join(' ')}\r\n`);
  process.exitCode = 1;
}
