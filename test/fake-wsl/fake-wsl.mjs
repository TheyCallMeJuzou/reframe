// Stand-in for wsl.exe in end-to-end tests. A distribution is a folder under REFRAME_WSL_SHARE (its root), with
// a user "ann"; its claude is the fake claude, run in the folder a Linux path names, with that distribution's
// ~/.claude, and shown in its /proc while it runs. Files in a distribution's root: "no-claude" (it has none),
// "stopped" (it does not run until something starts it). Understands what reframe uses:
//   -l -q / -l --running -q                                the distributions, all or the running ones
//   -d <name> -e sh -c <script>                            the probe (answered with what reframe asks for)
//   -d <name> --cd <dir> --exec env PATH=… <claude> <args>   claude
//   -d <name> -e kill -s <signal> <pid>                    ending a claude (as a signal would)
//   -d <name> -e sleep infinity                            keeping it running
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
const command = option('-e');
const exec = args.indexOf('--exec');

if (args[0] === '-l') {
  const distros = fs
    .readdirSync(share)
    .filter((name) => fs.statSync(path.join(share, name)).isDirectory())
    .filter((name) => !args.includes('--running') || !fs.existsSync(path.join(share, name, 'stopped')));
  process.stdout.write(distros.map((name) => `${name}\r\n`).join(''));
} else if (!distro || !fs.existsSync(path.join(share, distro))) {
  process.stderr.write('There is no distribution with the supplied name.\r\n');
  process.exitCode = 1;
} else if (command === 'sh') {
  const claude = fs.existsSync(path.join(share, distro, 'no-claude')) ? '' : '/usr/local/bin/claude';
  const answer = ['what a .bashrc prints', '@@REFRAME@@', '/usr/local/bin:/usr/bin', claude, HOME, '', 'MOUNT=/mnt/c/'];
  // A distribution takes a moment to start: reframe's windows are up before its sessions are.
  setTimeout(() => process.stdout.write(`${answer.join('\n')}\n`), 1500);
} else if (command === 'kill') {
  // The process ends, and with it its registry entry and its /proc folder (as Claude Code and Linux would see to).
  const pid = Number(args[args.length - 1]);
  try {
    process.kill(pid);
  } catch {
    // gone already
  }
  fs.rmSync(windowsPath(distro, `${HOME}/.claude/sessions/${pid}.json`), { force: true });
  fs.rmSync(windowsPath(distro, `/proc/${pid}`), { recursive: true, force: true });
} else if (command === 'sleep') {
  fs.rmSync(path.join(share, distro, 'stopped'), { force: true });
  setInterval(() => {}, 60_000);
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
      REFRAME_FAKE_PROC: windowsPath(distro, '/proc'),
    },
  });
  child.on('exit', (code) => process.exit(code ?? 1));
} else {
  process.stderr.write(`fake wsl.exe does not know: ${args.join(' ')}\r\n`);
  process.exitCode = 1;
}
