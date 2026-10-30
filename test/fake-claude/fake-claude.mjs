// Stand-in for the claude CLI in end-to-end tests. Behaves like the parts reframe relies on:
// --session-id / --resume, HTTP hooks from --settings, the session registry file, transcripts that
// only exist from the first prompt on, /clear (new session id, no hook), /mouse (mouse reporting, as in Claude
// Code's fullscreen mode, with its own paste on a right-click) and Ctrl+C twice to exit.
// Every launch is appended to REFRAME_FAKE_LOG. Started by the WSL stand-in, it has no hooks (no --settings) and
// registers the Linux folder it was given (REFRAME_FAKE_CWD).
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const args = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const home = process.env.REFRAME_CLAUDE_HOME;
const log = process.env.REFRAME_FAKE_LOG;
const resumeId = option('--resume');
let sessionId = resumeId ?? option('--session-id') ?? crypto.randomUUID();
const name = option('--name') ?? `fake-${process.pid}`;
const settings = option('--settings') ? JSON.parse(fs.readFileSync(option('--settings'), 'utf8')) : {};
// --worktree [name]: like Claude Code, it makes <folder>/.claude/worktrees/<name> and runs in there.
const worktree = args.includes('--worktree')
  ? (option('--worktree')?.startsWith('-') ? undefined : option('--worktree')) || `wt-${process.pid}`
  : undefined;

const transcript = (id) => path.join(home, 'projects', 'fake-project', `${id}.jsonl`);
const registryFile = path.join(home, 'sessions', `${process.pid}.json`);

function record(event) {
  const script = process.argv[1];
  if (log) fs.appendFileSync(log, JSON.stringify({ pid: process.pid, event, args, cwd: process.cwd(), script }) + '\n');
}

function writeRegistry(status) {
  fs.mkdirSync(path.dirname(registryFile), { recursive: true });
  const nameSource = option('--name') ? 'user' : 'auto';
  const cwd = process.env.REFRAME_FAKE_CWD ?? process.cwd();
  const entry = { pid: process.pid, sessionId, cwd, name, nameSource, status };
  fs.writeFileSync(registryFile, JSON.stringify(entry));
}

async function hook(event, extra = {}) {
  const entry = settings.hooks?.[event]?.[0]?.hooks?.[0];
  if (!entry) return;
  await fetch(entry.url, {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...entry.headers },
    body: JSON.stringify({ session_id: sessionId, cwd: process.cwd(), hook_event_name: event, ...extra }),
  }).catch(() => {});
}

// In a WSL stand-in's distribution, the process shows in its /proc while it runs.
const procFolder = process.env.REFRAME_FAKE_PROC && path.join(process.env.REFRAME_FAKE_PROC, String(process.pid));

function exit(code) {
  fs.rmSync(registryFile, { force: true });
  if (procFolder) fs.rmSync(procFolder, { recursive: true, force: true });
  record('exit');
  process.exit(code);
}

record('start');
if (worktree) {
  const folder = path.join(process.cwd(), '.claude', 'worktrees', worktree);
  fs.mkdirSync(folder, { recursive: true });
  process.chdir(folder);
}
if (resumeId && !fs.existsSync(transcript(resumeId))) {
  process.stdout.write(`No conversation found with session ID: ${resumeId}\r\n`);
  exit(1);
}
writeRegistry('idle');
if (procFolder) {
  fs.mkdirSync(procFolder, { recursive: true });
  // As /proc/<pid>/cmdline has it: the distribution's claude and its arguments, NUL-separated.
  fs.writeFileSync(path.join(procFolder, 'cmdline'), ['/usr/local/bin/claude', ...args].join('\0'));
}
process.stdout.write(`\x1b[1mFAKE CLAUDE\x1b[0m ${resumeId ? 'resumed' : 'new'} ${sessionId}\r\n> `);

let line = '';
// "/ask": like a permission prompt, the session waits for the user until the next Enter.
let asking = false;
let lastInterrupt = 0;
let images = 0;
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stdin.on('data', async (input) => {
  // After "/mouse", mouse reports come in: a plain right-button press stands for the paste Claude Code's fullscreen
  // mode does on its own, the others are dropped.
  // eslint-disable-next-line no-control-regex -- a mouse report starts with ESC
  const typed = input.replace(/\x1b\[<(\d+);\d+;\d+([Mm])/g, (_report, button, end) =>
    button === '2' && end === 'M' ? '[right click]' : '',
  );
  // Claude Code pastes the clipboard's image on Alt+V (Windows) or Ctrl+V and shows a chip for it.
  const chunk = ['\x1bv', '\x16'].reduce((text, key) => text.replaceAll(key, () => `[Image #${++images}]`), typed);
  for (const ch of chunk) {
    if (ch === '\x03') {
      if (Date.now() - lastInterrupt < 2000) exit(0);
      lastInterrupt = Date.now();
      process.stdout.write('\r\nPress Ctrl-C again to exit\r\n> ');
      line = '';
    } else if (ch === '\r') {
      const prompt = line;
      line = '';
      process.stdout.write('\r\n');
      if (asking) {
        asking = false;
        writeRegistry('idle');
        await hook('Stop');
        process.stdout.write('allowed\r\n> ');
        continue;
      }
      if (prompt === '/ask') {
        asking = true;
        await hook('UserPromptSubmit', { prompt });
        fs.mkdirSync(path.dirname(transcript(sessionId)), { recursive: true });
        fs.appendFileSync(transcript(sessionId), JSON.stringify({ type: 'user', prompt }) + '\n');
        writeRegistry('waiting');
        await hook('Notification', { notification_type: 'permission_prompt', message: 'Claude needs your permission' });
        process.stdout.write('Allow? (Enter)\r\n');
        continue;
      }
      if (prompt === '/mouse') {
        // Mouse reporting (presses and releases, SGR), as Claude Code's fullscreen mode turns on.
        process.stdout.write('\x1b[?1000h\x1b[?1006hmouse on\r\n> ');
        continue;
      }
      if (prompt === '/clear') {
        // Local slash commands fire no hooks; the new session has no transcript yet.
        sessionId = crypto.randomUUID();
        writeRegistry('idle');
        process.stdout.write(`cleared, new session ${sessionId}\r\n> `);
        continue;
      }
      await hook('UserPromptSubmit', { prompt });
      writeRegistry('busy');
      fs.mkdirSync(path.dirname(transcript(sessionId)), { recursive: true });
      fs.appendFileSync(transcript(sessionId), JSON.stringify({ type: 'user', prompt }) + '\n');
      process.stdout.write(`echo: ${prompt}\r\n`);
      // Without hooks (WSL) only the registry tells: busy for a moment, as a real answer takes, so polling sees it.
      if (!option('--settings')) await new Promise((resolve) => setTimeout(resolve, 1500));
      writeRegistry('idle');
      await hook('Stop');
      process.stdout.write('> ');
    } else if (ch === '\x7f') {
      line = line.slice(0, -1);
      process.stdout.write('\b \b');
    } else {
      line += ch;
      process.stdout.write(ch);
    }
  }
});
