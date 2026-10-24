// Stand-in for the claude CLI in end-to-end tests. Behaves like the parts reframe relies on:
// --session-id / --resume, HTTP hooks from --settings, the session registry file, transcripts that
// only exist from the first prompt on, /clear (new session id, no hook) and Ctrl+C twice to exit.
// Every launch is appended to REFRAME_FAKE_LOG.
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
const settings = JSON.parse(fs.readFileSync(option('--settings'), 'utf8'));

const transcript = (id) => path.join(home, 'projects', 'fake-project', `${id}.jsonl`);
const registryFile = path.join(home, 'sessions', `${process.pid}.json`);

function record(event) {
  if (log) fs.appendFileSync(log, JSON.stringify({ pid: process.pid, event, args, cwd: process.cwd() }) + '\n');
}

function writeRegistry(status) {
  fs.mkdirSync(path.dirname(registryFile), { recursive: true });
  const nameSource = option('--name') ? 'user' : 'auto';
  const entry = { pid: process.pid, sessionId, cwd: process.cwd(), name, nameSource, status };
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

function exit(code) {
  fs.rmSync(registryFile, { force: true });
  record('exit');
  process.exit(code);
}

record('start');
if (resumeId && !fs.existsSync(transcript(resumeId))) {
  process.stdout.write(`No conversation found with session ID: ${resumeId}\r\n`);
  exit(1);
}
writeRegistry('idle');
process.stdout.write(`\x1b[1mFAKE CLAUDE\x1b[0m ${resumeId ? 'resumed' : 'new'} ${sessionId}\r\n> `);

let line = '';
let lastInterrupt = 0;
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stdin.on('data', async (chunk) => {
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
