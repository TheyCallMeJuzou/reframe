// Stand-in for the claude CLI when taking the README's screenshots: draws the conversation scenes.mjs has for its
// --name the way Claude Code looks, and takes input like it does: typed text, [Image #n] for a pasted or dropped
// image, [Pasted text #n] for long pastes. Enter plays the scene's scripted reply. Like the test fake, it keeps a
// session registry entry (with the scene's status) and exits on Ctrl+C twice.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { scenes } from './scenes.mjs';

const args = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const sessionId = option('--resume') ?? option('--session-id') ?? crypto.randomUUID();
const name = option('--name') ?? path.basename(process.cwd());
const scene = scenes[name] ?? { status: 'idle', blocks: [] };
const registryFile = path.join(process.env.REFRAME_CLAUDE_HOME, 'sessions', `${process.pid}.json`);

let status = scene.status;
let blocks = [...scene.blocks];
let spinner = status === 'busy' ? scene.spinner : undefined;
let permission = status === 'waiting' ? scene.permission : undefined;
let input = [...(scene.draft ?? '')];
let images = 0;
let pastes = 0;
const startedAt = Date.now();

function writeRegistry() {
  fs.mkdirSync(path.dirname(registryFile), { recursive: true });
  const entry = { pid: process.pid, sessionId, cwd: process.cwd(), name, nameSource: 'user', status };
  fs.writeFileSync(registryFile, JSON.stringify(entry));
}

// ---------- drawing ----------

const rgb = (r, g, b) => `\x1b[38;2;${r};${g};${b}m`;
const onRgb = (r, g, b) => `\x1b[48;2;${r};${g};${b}m`;
const RESET = '\x1b[0m';
const BOLD = '\x1b[1m';
const STRIKE = '\x1b[9m';
const INVERSE = '\x1b[7m';
const C = {
  text: rgb(228, 230, 235),
  dim: rgb(138, 143, 152),
  accent: rgb(215, 119, 87),
  ok: rgb(78, 186, 101),
  code: rgb(177, 185, 249),
  permission: rgb(177, 185, 249),
  accept: rgb(175, 135, 255),
  userBg: onRgb(44, 46, 52),
  removedBg: onRgb(92, 32, 40),
  addedBg: onRgb(30, 76, 44),
};
const SPINNER = ['✢', '✳', '✶', '✻', '✽', '✻', '✶', '✳', '✢'];

/** One screen line, cut off with "…" where it would run past `width`. */
class Line {
  constructor(width) {
    this.width = width;
    this.out = '';
    this.n = 0;
  }
  add(style, text) {
    const room = this.width - this.n;
    if (room <= 0 || !text) return this;
    const cut = text.length > room ? `${text.slice(0, Math.max(0, room - 1))}…` : text;
    this.out += style + cut;
    this.n += cut.length;
    return this;
  }
  /** Fills the rest of the line, e.g. with a background color. */
  fill(style) {
    if (this.n < this.width) this.out += style + ' '.repeat(this.width - this.n);
    this.n = this.width;
    return this;
  }
  toString() {
    return `${RESET}${this.out}${RESET}`;
  }
}

/** Text wrapped at word boundaries after a first-line and a following-lines prefix; `code` in the code color. */
function paragraph(text, width, first, rest, style, bg = '') {
  const chars = [];
  let code = false;
  for (const ch of text) {
    if (ch === '`') code = !code;
    else chars.push({ ch, code });
  }
  const words = [];
  let word = [];
  for (const c of chars) {
    if (c.ch === ' ') {
      if (word.length) words.push(word);
      word = [];
    } else word.push(c);
  }
  if (word.length) words.push(word);

  const lines = [];
  let line = new Line(width).add(bg + first[0], first[1]);
  let empty = true;
  const put = (part) => {
    let run = '';
    let runCode = part[0]?.code;
    for (const c of part) {
      if (c.code !== runCode) {
        line.add(bg + (runCode ? C.code : style), run);
        run = '';
        runCode = c.code;
      }
      run += c.ch;
    }
    line.add(bg + (runCode ? C.code : style), run);
  };
  for (const w of words) {
    const room = width - line.n - (empty ? 0 : 1);
    if (w.length > room && !empty) {
      lines.push(bg ? line.fill(bg) : line);
      line = new Line(width).add(bg + rest[0], rest[1]);
      empty = true;
    }
    if (!empty) line.add(bg + style, ' ');
    put(w);
    empty = false;
  }
  lines.push(bg ? line.fill(bg) : line);
  return lines.map(String);
}

function tool(block, width) {
  const lines = [];
  const bullet = block.running ? C.dim : C.ok;
  lines.push(
    String(
      new Line(width)
        .add(bullet, '● ')
        .add(C.text + BOLD, block.tool)
        .add(RESET + C.text, `(${block.arg})`),
    ),
  );
  const results = block.running ? ['Running…'] : (block.result ?? []);
  results.forEach((text, i) =>
    lines.push(String(new Line(width).add(C.dim, i === 0 ? '  ⎿  ' : '     ').add(C.dim, text))),
  );
  for (const [number, sign, code] of block.diff ?? []) {
    const bg = sign === '-' ? C.removedBg : sign === '+' ? C.addedBg : '';
    const line = new Line(width)
      .add(RESET, '      ')
      .add(bg + C.dim, String(number).padStart(4))
      .add(bg + C.text, ` ${sign}  ${code}`);
    lines.push(String(bg ? line.fill(bg) : line));
  }
  return lines;
}

function todos(block, width) {
  const lines = [String(new Line(width).add(C.ok, '● ').add(C.text + BOLD, 'Update Todos'))];
  block.todos.forEach(([state, text], i) => {
    const line = new Line(width).add(C.dim, i === 0 ? '  ⎿  ' : '     ');
    if (state === 'done') line.add(C.dim, '☒ ').add(C.dim + STRIKE, text);
    else if (state === 'doing') line.add(C.text + BOLD, `☐ ${text}`);
    else line.add(C.text, `☐ ${text}`);
    lines.push(String(line));
  });
  return lines;
}

function render(block, width) {
  if (block.user !== undefined) return paragraph(block.user, width, [C.dim, '> '], [C.dim, '  '], C.text, C.userBg);
  if (block.say !== undefined) return paragraph(block.say, width, [C.text, '● '], [C.text, '  '], C.text);
  if (block.todos) return todos(block, width);
  return tool(block, width);
}

function footer(width) {
  const lines = [];
  if (spinner) {
    const tick = Math.floor((Date.now() - startedAt) / 120);
    const seconds = spinner.seconds + Math.floor((Date.now() - startedAt) / 1000);
    lines.push(
      String(
        new Line(width)
          .add(C.accent, `${SPINNER[tick % SPINNER.length]} ${spinner.verb} `)
          .add(C.dim, `(${seconds}s · ↓ ${spinner.tokens} tokens · esc to interrupt)`),
      ),
      '',
    );
  }
  const rule = (style) => String(new Line(width).add(style, '─'.repeat(width)));
  if (permission) {
    lines.push(rule(C.permission));
    lines.push(String(new Line(width).add(C.permission + BOLD, ` ${permission.title}`)), '');
    lines.push(String(new Line(width).add(C.text, `   ${permission.command}`)));
    lines.push(String(new Line(width).add(C.dim, `   ${permission.description}`)), '');
    lines.push(String(new Line(width).add(C.text, ' Do you want to proceed?')));
    permission.options.forEach((text, i) =>
      lines.push(
        String(
          i === 0
            ? new Line(width).add(C.permission, ` ❯ ${i + 1}. ${text}`)
            : new Line(width).add(C.text, `   ${i + 1}. ${text}`),
        ),
      ),
    );
    return lines;
  }
  const typed = input.join('');
  const room = width - 3;
  const shown = typed.length > room ? typed.slice(typed.length - room) : typed;
  lines.push(rule(C.dim));
  lines.push(String(new Line(width).add(C.dim, '> ').add(C.text, shown).add(INVERSE, ' ')));
  lines.push(rule(C.dim));
  lines.push(
    String(
      scene.hint === 'accept'
        ? new Line(width).add(C.accept, '  ⏵⏵ accept edits on ').add(C.dim, '(shift+tab to cycle)')
        : new Line(width).add(C.dim, '  ? for shortcuts'),
    ),
  );
  return lines;
}

function draw() {
  const width = Math.max(20, process.stdout.columns || 80);
  const height = Math.max(5, process.stdout.rows || 24);
  const content = blocks.flatMap((block, i) => [...(i > 0 ? [''] : []), ...render(block, width)]);
  let rows = [...content, ...(content.length ? [''] : []), ...footer(width)];
  if (rows.length > height) rows = rows.slice(rows.length - height);
  let out = '\x1b[?25l';
  for (let i = 0; i < height; i++) out += `\x1b[${i + 1};1H\x1b[2K${rows[i] ?? ''}`;
  process.stdout.write(out);
}

// ---------- input ----------

/** A paste or drop of image files becomes [Image #n] chips, as in Claude Code; null if it holds anything else. */
function imageChips(text) {
  const parts = text.trim().match(/"[^"]+"|'[^']+'|\S+/g) ?? [];
  const isImage = (part) => /\.(png|jpe?g|gif|webp)["']?$/i.test(part);
  return parts.length > 0 && parts.every(isImage) ? parts.map(() => `[Image #${++images}]`) : null;
}

let lastInterrupt = 0;
function key(ch) {
  if (ch === '\x03') {
    if (Date.now() - lastInterrupt < 2000) exit();
    lastInterrupt = Date.now();
  } else if (ch === '\r') submit();
  else if (ch === '\x7f' || ch === '\b') input.pop();
  else if (ch >= ' ') input.push(ch);
}

function take(chunk) {
  if (chunk === '\x1bv' || chunk === '\x16') input.push(`[Image #${++images}] `);
  else if (chunk.startsWith('\x1b'))
    return; // arrow keys and the like
  else if (chunk.length > 1 && /[\r\n]./.test(chunk)) {
    const count = chunk.split(/\r\n|\r|\n/).length;
    input.push(`[Pasted text #${++pastes} +${count} lines] `);
  } else {
    const chips = chunk.length > 1 && imageChips(chunk);
    if (chips) input.push(chips.join(' ') + ' ');
    else for (const ch of chunk) key(ch);
  }
  draw();
}

let reply = scene.reply;
function submit() {
  const text = input.join('').trim();
  if (!text || permission) return;
  input = [];
  blocks = [...blocks, { user: text }];
  if (!reply) return;
  const { spinner: thinking, blocks: answer } = reply;
  reply = undefined;
  status = 'busy';
  spinner = thinking;
  writeRegistry();
  answer.forEach((block, i) =>
    setTimeout(
      () => {
        blocks = [...blocks, block];
        if (i === answer.length - 1) {
          status = 'idle';
          spinner = undefined;
          writeRegistry();
        }
        draw();
      },
      1400 + i * 700,
    ),
  );
}

function exit() {
  fs.rmSync(registryFile, { force: true });
  process.exit(0);
}

// Keys come one by one; a paste comes at once but may arrive in pieces, so input is taken in short bursts.
let pending = '';
let flush = null;
if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
process.stdin.on('data', (data) => {
  pending += data;
  clearTimeout(flush);
  flush = setTimeout(() => {
    const chunk = pending;
    pending = '';
    take(chunk);
  }, 25);
});
process.stdout.on('resize', draw);
writeRegistry();
draw();
setInterval(() => spinner && draw(), 120);
