// Phase-0 spike: one claude session lives in a utilityProcess PTY host and can be shown in any window.
// Manual mode: drag the tile header between windows or into empty desktop space; drag foreign windows onto ours.
// Auto mode (--spike-auto): scripted run that captures screenshots and logs findings to output/.
const { app, BrowserWindow, utilityProcess, MessageChannelMain, ipcMain, screen } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
const http = require('node:http');
const crypto = require('node:crypto');
const { execFile } = require('node:child_process');

const AUTO = process.argv.includes('--spike-auto');
const cwdArg = process.argv.find((a) => a.startsWith('--cwd='));
const SESSION_CWD = cwdArg ? cwdArg.slice('--cwd='.length) : process.cwd();
const OUT = path.join(__dirname, 'output');
fs.mkdirSync(OUT, { recursive: true });
const LOG = path.join(OUT, 'spike.log');
fs.writeFileSync(LOG, '');

function log(...parts) {
  const line = `[${new Date().toISOString()}] ${parts.map((p) => (typeof p === 'string' ? p : JSON.stringify(p))).join(' ')}`;
  console.log(line);
  fs.appendFileSync(LOG, line + '\n');
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- hook server ----------
const HOOK_TOKEN = crypto.randomBytes(16).toString('hex');
const hookEvents = [];
let hookPort = 0;
function startHookServer() {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      let body = '';
      req.on('data', (c) => (body += c));
      req.on('end', () => {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end('{}');
        if (req.headers['x-reframe-token'] !== HOOK_TOKEN) return log('hook: rejected (bad token)');
        let payload = {};
        try { payload = JSON.parse(body); } catch {}
        const ev = { via: 'http', path: req.url, event: payload.hook_event_name, session_id: payload.session_id,
          source: payload.source, notification_type: payload.notification_type, message: payload.message, cwd: payload.cwd };
        hookEvents.push(ev);
        log('hook:', ev);
      });
    });
    server.listen(0, '127.0.0.1', () => {
      hookPort = server.address().port;
      resolve();
    });
  });
}

function writeSessionSettings(tileId) {
  const httpHook = (name) => ({ type: 'http', url: `http://127.0.0.1:${hookPort}/hook/${tileId}/${name}`, headers: { 'X-Reframe-Token': HOOK_TOKEN } });
  const cmd = `node "${path.join(__dirname, 'cmdhook.cjs').replace(/\\/g, '/')}"`;
  const settings = {
    hooks: {
      SessionStart: [{ hooks: [httpHook('SessionStart'), { type: 'command', command: cmd }] }],
      UserPromptSubmit: [{ hooks: [httpHook('UserPromptSubmit')] }],
      Stop: [{ hooks: [httpHook('Stop')] }],
      Notification: [{ hooks: [httpHook('Notification')] }],
    },
  };
  const file = path.join(OUT, `settings-${tileId}.json`);
  fs.writeFileSync(file, JSON.stringify(settings, null, 2));
  return file;
}

// ---------- claude binary ----------
function resolveClaude() {
  // Follow PATH + PATHEXT like cmd.exe; unwrap npm cmd-shims to the real exe so the PTY pid is claude itself.
  const exts = process.platform === 'win32' ? (process.env.PATHEXT || '.EXE;.CMD').split(';') : [''];
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    for (const ext of exts) {
      const candidate = path.join(dir, 'claude' + ext.toLowerCase());
      if (!fs.existsSync(candidate)) continue;
      if (/\.cmd$/i.test(candidate)) {
        const m = fs.readFileSync(candidate, 'utf8').match(/"%dp0%\\([^"]+\.exe)"/i);
        if (m) return { file: path.join(path.dirname(candidate), m[1]), via: candidate };
        return { file: process.env.ComSpec || 'cmd.exe', prefix: ['/d', '/s', '/c', candidate], via: candidate };
      }
      return { file: candidate, via: candidate };
    }
  }
  throw new Error('claude not found on PATH');
}

// Markers a parent Claude Code session puts into its children's env. Inherited by a spawned claude,
// they turn it into a "child session" (no transcript, no registry entry), so they must not leak through.
const PARENT_SESSION_ENV = /^(CLAUDECODE|CLAUDE_PID|CLAUDE_CODE_(CHILD_SESSION|SESSION_ID|SESSION_ATTENDED|BRIDGE_SESSION_ID|MESSAGING_SOCKET|MESSAGING_TOKEN|ENTRYPOINT))$/;
function cleanEnv(extra) {
  const env = {};
  for (const [k, v] of Object.entries(process.env)) if (!PARENT_SESSION_ENV.test(k)) env[k] = v;
  return { ...env, ...extra };
}

const TILE_ID = 'tile-1';
function spawnClaude(sessionArgs) {
  const claude = resolveClaude();
  const settings = writeSessionSettings(TILE_ID);
  const args = [...(claude.prefix ?? []), ...sessionArgs, '--settings', settings, '--model', 'haiku', '--permission-mode', 'manual'];
  log('claude:', claude, 'args:', args, 'cwd:', SESSION_CWD);
  host.postMessage({ type: 'spawn', file: claude.file, args, cwd: SESSION_CWD, env: cleanEnv({ REFRAME_TILE: TILE_ID, REFRAME_SPIKE_OUT: OUT }), cols: 120, rows: 32 });
}

function transcriptExists(sessionId) {
  const projects = path.join(os.homedir(), '.claude', 'projects');
  for (const dir of fs.readdirSync(projects)) if (fs.existsSync(path.join(projects, dir, `${sessionId}.jsonl`))) return path.join(dir, `${sessionId}.jsonl`);
  return false;
}

// ---------- pty host ----------
let host;
let claudePid = 0;
let exited = null;
const pendingSerialize = new Map();
function startPtyHost() {
  host = utilityProcess.fork(path.join(__dirname, 'pty-host.js'), [], { serviceName: 'reframe-pty-host' });
  host.on('message', (msg) => {
    if (msg.type === 'spawned') { claudePid = msg.pid; log('pty: spawned pid', msg.pid); }
    if (msg.type === 'exit') { exited = msg; log('pty: exit', msg); }
    if (msg.type === 'serialized') { pendingSerialize.get(msg.reqId)?.(msg); pendingSerialize.delete(msg.reqId); }
  });
}
function serialize() {
  const reqId = crypto.randomUUID();
  return new Promise((resolve) => {
    pendingSerialize.set(reqId, resolve);
    host.postMessage({ type: 'serialize', reqId });
  });
}
const write = (data) => host.postMessage({ type: 'write', data });

// ---------- windows ----------
const windows = new Map(); // id -> BrowserWindow
let owner = null; // window id currently showing the session
let nextWinId = 1;

function createWindow(bounds = {}) {
  const id = nextWinId++;
  const win = new BrowserWindow({
    width: bounds.width ?? 1000, height: bounds.height ?? 640, x: bounds.x, y: bounds.y,
    title: `reframe spike – window ${id}`, show: false, backgroundColor: '#1e1e1e',
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: false },
  });
  windows.set(id, win);
  win.loadFile(path.join(__dirname, 'index.html'), { query: { win: String(id) } });
  win.once('ready-to-show', () => (AUTO ? win.showInactive() : win.show()));
  win.on('closed', () => windows.delete(id));
  return { id, win, ready: new Promise((r) => win.webContents.once('did-finish-load', r)) };
}

function attachTo(id) {
  const target = windows.get(id);
  if (!target) return;
  if (owner !== null && owner !== id) windows.get(owner)?.webContents.send('detach');
  owner = id;
  const { port1, port2 } = new MessageChannelMain();
  host.postMessage({ type: 'attach' }, [port1]);
  target.webContents.postMessage('terminal-port', null, [port2]);
  for (const [wid, w] of windows) w.webContents.send('owner', { owner, me: wid });
  log('attach: session now shown in window', id);
}

function windowAtPoint(pt) {
  for (const [id, w] of windows) {
    const b = w.getBounds();
    if (pt.x >= b.x && pt.x < b.x + b.width && pt.y >= b.y && pt.y < b.y + b.height) return id;
  }
  return null;
}

ipcMain.on('drag-start', (e, info) => log('dnd: dragstart', info));
ipcMain.on('tile-dropped', (e, { targetWin, data }) => {
  log('dnd: drop in window', targetWin, 'data:', data);
  if (targetWin !== owner) attachTo(targetWin);
});
ipcMain.on('drag-end', async (e, { dropEffect }) => {
  const pt = screen.getCursorScreenPoint();
  const over = windowAtPoint(pt);
  log('dnd: dragend dropEffect=', dropEffect, 'cursor=', pt, 'overWindow=', over);
  if (dropEffect === 'none' && over === null) {
    const { id, ready } = createWindow({ x: pt.x - 100, y: pt.y - 20 });
    await ready;
    attachTo(id);
    log('dnd: torn out into new window', id);
  }
});

// ---------- foreign window drag (Windows, koffi) ----------
let winEventCallbacks = [];
function installWinEventHooks() {
  if (process.platform !== 'win32') return log('winevent: skipped (not win32)');
  const koffi = require('koffi');
  const user32 = koffi.load('user32.dll');
  const WINEVENTPROC = koffi.proto('void __stdcall WINEVENTPROC(void *hWinEventHook, uint32_t event, void *hwnd, long idObject, long idChild, uint32_t idEventThread, uint32_t dwmsEventTime)');
  const SetWinEventHook = user32.func('void * __stdcall SetWinEventHook(uint32_t eventMin, uint32_t eventMax, void *hmod, WINEVENTPROC *pfn, uint32_t idProcess, uint32_t idThread, uint32_t dwFlags)');
  const GetWindowThreadProcessId = user32.func('uint32_t __stdcall GetWindowThreadProcessId(void *hWnd, _Out_ uint32_t *lpdwProcessId)');
  const GetWindowTextW = user32.func('int __stdcall GetWindowTextW(void *hWnd, void *lpString, int nMaxCount)');

  const describe = (hwnd) => {
    const pid = [0];
    GetWindowThreadProcessId(hwnd, pid);
    const buf = Buffer.alloc(1024);
    const n = GetWindowTextW(hwnd, buf, 512);
    return { hwnd: String(hwnd), pid: pid[0], title: buf.toString('utf16le', 0, n * 2) };
  };

  const EVENT_SYSTEM_MOVESIZESTART = 0x000a, EVENT_SYSTEM_MOVESIZEEND = 0x000b, EVENT_OBJECT_NAMECHANGE = 0x800c;
  const WINEVENT_SKIPOWNPROCESS = 0x0002;
  let nameChanges = 0;

  const onMoveSize = koffi.register((hook, event, hwnd, idObject) => {
    if (idObject !== 0) return;
    const info = describe(hwnd);
    const pt = screen.getCursorScreenPoint();
    const over = windowAtPoint(pt);
    if (event === EVENT_SYSTEM_MOVESIZESTART) return log('winevent: foreign drag start', info);
    log('winevent: foreign drag end', info, 'cursor=', pt, 'overReframeWindow=', over);
    if (over !== null) probeProcessTree(info.pid).then((r) => log('takeover probe:', r));
  }, koffi.pointer(WINEVENTPROC));

  const onNameChange = koffi.register((hook, event, hwnd, idObject) => {
    if (idObject !== 0) return;
    nameChanges++;
    if (nameChanges <= 3) log('winevent: own NAMECHANGE received (callback path works)', describe(hwnd));
  }, koffi.pointer(WINEVENTPROC));

  const h1 = SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, null, onMoveSize, 0, 0, WINEVENT_SKIPOWNPROCESS);
  const h2 = SetWinEventHook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE, null, onNameChange, process.pid, 0, 0);
  winEventCallbacks = [onMoveSize, onNameChange];
  log('winevent: hooks installed', { movesize: String(h1), namechange: String(h2) });
}

function probeProcessTree(rootPid) {
  // Snapshot all processes, walk descendants of the dragged window's process, keep claude ones,
  // and enrich them from Claude Code's (undocumented) ~/.claude/sessions/<pid>.json registry.
  const ps = 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress';
  return new Promise((resolve) => {
    execFile('powershell.exe', ['-NoProfile', '-Command', ps], { maxBuffer: 64 * 1024 * 1024 }, (err, stdout) => {
      if (err) return resolve({ error: String(err) });
      const procs = JSON.parse(stdout);
      const children = new Map();
      for (const p of procs) {
        if (!children.has(p.ParentProcessId)) children.set(p.ParentProcessId, []);
        children.get(p.ParentProcessId).push(p);
      }
      const found = [];
      const stack = [rootPid];
      while (stack.length) {
        const pid = stack.pop();
        for (const c of children.get(pid) ?? []) {
          if (/^claude(\.exe)?$/i.test(c.Name)) found.push({ pid: c.ProcessId, cmd: c.CommandLine, registry: readRegistry(c.ProcessId) });
          stack.push(c.ProcessId);
        }
      }
      resolve({ rootPid, claudeProcesses: found });
    });
  });
}

function readRegistry(pid) {
  const file = path.join(os.homedir(), '.claude', 'sessions', `${pid}.json`);
  try {
    const j = JSON.parse(fs.readFileSync(file, 'utf8'));
    return { sessionId: j.sessionId, cwd: j.cwd, name: j.name, status: j.status, kind: j.kind, entrypoint: j.entrypoint };
  } catch (e) {
    return { missing: file };
  }
}

// ---------- capture helpers ----------
async function capture(id, name) {
  const w = windows.get(id);
  if (!w) return log('capture: window missing', id);
  const img = await w.webContents.capturePage();
  fs.writeFileSync(path.join(OUT, `${name}.png`), img.toPNG());
  log('capture:', `${name}.png`, img.getSize());
}
async function dumpText(name) {
  const s = await serialize();
  fs.writeFileSync(path.join(OUT, `${name}.txt`), s.text);
  log(`serialize: ${name}`, { ansiBytes: s.data.length, textLines: s.text.split('\n').length });
  return s;
}
async function waitFor(pred, ms) {
  const end = Date.now() + ms;
  while (Date.now() < end) { if (pred()) return true; await sleep(250); }
  return false;
}
async function typeLine(text) {
  write(text);
  await sleep(600);
  write('\r');
}

async function autoRun(winA) {
  await sleep(9000);
  await capture(winA, 'A-1-start');
  let s = await dumpText('A-1-start');
  log('registry after start:', readRegistry(claudePid));
  if (/trust/i.test(s.text)) {
    log('auto: trust dialog detected, confirming');
    write('\r');
    await sleep(6000);
    await capture(winA, 'A-2-after-trust');
    await dumpText('A-2-after-trust');
  }

  log('auto: moving session to a new window B');
  const { id: winB, ready } = createWindow({ width: 1200, height: 700 });
  await ready;
  attachTo(winB);
  await sleep(3500);
  await capture(winB, 'B-1-moved');
  await capture(winA, 'A-3-after-move');
  await dumpText('B-1-moved');

  const before = readRegistry(claudePid);
  log('auto: /clear');
  await typeLine('/clear');
  await sleep(5000);
  log('registry before /clear:', before, 'after:', readRegistry(claudePid));
  await capture(winB, 'B-2-after-clear');

  log('auto: prompt that needs a permission');
  const n0 = hookEvents.length;
  await typeLine('Create a file named reframe-spike.txt containing the word hi. Do nothing else.');
  const gotNotification = await waitFor(() => hookEvents.slice(n0).some((e) => e.event === 'Notification' || e.event === 'Stop'), 90000);
  log('auto: notification/stop received?', gotNotification);
  log('registry while waiting:', readRegistry(claudePid));
  await capture(winB, 'B-3-permission');
  await dumpText('B-3-permission');
  write('\x1b');
  await sleep(3000);

  log('auto: exiting claude with Ctrl+C x2');
  write('\x03');
  await sleep(700);
  write('\x03');
  const didExit = await waitFor(() => exited !== null, 15000);
  log('auto: claude exited?', didExit, exited);
  await capture(winB, 'B-4-exit');

  const lastId = [...hookEvents].reverse().find((e) => e.session_id)?.session_id;
  log('auto: transcript exists for', lastId, '?', transcriptExists(lastId));
  log('auto: resuming', lastId, 'in', SESSION_CWD);
  exited = null;
  spawnClaude(['--resume', lastId]);
  attachTo(winB);
  await sleep(10000);
  log('registry after resume:', readRegistry(claudePid));
  await capture(winB, 'B-5-resumed');
  await dumpText('B-5-resumed');
  write('\x03');
  await sleep(700);
  write('\x03');
  await waitFor(() => exited !== null, 15000);

  fs.writeFileSync(path.join(OUT, 'hook-events.json'), JSON.stringify(hookEvents, null, 2));
  log('auto: done');
  setTimeout(() => app.quit(), 500);
}

app.whenReady().then(async () => {
  await startHookServer();
  startPtyHost();
  installWinEventHooks();

  spawnClaude(['--session-id', crypto.randomUUID()]);

  const { id: winA, win, ready } = createWindow();
  await ready;
  attachTo(winA);
  win.setTitle('reframe spike – window 1 (session owner)');
  if (AUTO) autoRun(winA).catch((e) => { log('auto: failed', String(e?.stack ?? e)); app.quit(); });
});

app.on('window-all-closed', () => {
  host?.postMessage({ type: 'kill' });
  app.quit();
});
