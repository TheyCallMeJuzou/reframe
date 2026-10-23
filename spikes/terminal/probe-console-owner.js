// Spike 5b: map running claude sessions to the terminal window that hosts them.
// For every claude process with a registry entry: attach to its console, take the console window
// (ConPTY pseudo window under Windows Terminal, the real window under conhost) and walk to its owner.
// Read-only: we only attach, query and detach; nothing is written to the sessions.
// Run with plain node: node probe-console-owner.js
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const koffi = require('koffi');

const kernel32 = koffi.load('kernel32.dll');
const user32 = koffi.load('user32.dll');
const FreeConsole = kernel32.func('bool __stdcall FreeConsole()');
const AttachConsole = kernel32.func('bool __stdcall AttachConsole(uint32_t dwProcessId)');
const GetConsoleWindow = kernel32.func('void * __stdcall GetConsoleWindow()');
const GetLastError = kernel32.func('uint32_t __stdcall GetLastError()');
const GetWindow = user32.func('void * __stdcall GetWindow(void *hWnd, uint32_t uCmd)');
const GetAncestor = user32.func('void * __stdcall GetAncestor(void *hWnd, uint32_t gaFlags)');
const GetWindowThreadProcessId = user32.func('uint32_t __stdcall GetWindowThreadProcessId(void *hWnd, _Out_ uint32_t *lpdwProcessId)');
const GetWindowTextW = user32.func('int __stdcall GetWindowTextW(void *hWnd, void *lpString, int nMaxCount)');
const IsWindowVisible = user32.func('bool __stdcall IsWindowVisible(void *hWnd)');
const WNDENUMPROC = koffi.proto('bool __stdcall WNDENUMPROC(void *hwnd, intptr_t lParam)');
const EnumWindows = user32.func('bool __stdcall EnumWindows(WNDENUMPROC *cb, intptr_t lParam)');
const GW_OWNER = 4;
const GA_ROOTOWNER = 3;

function describe(hwnd) {
  if (!hwnd) return null;
  const pid = [0];
  GetWindowThreadProcessId(hwnd, pid);
  const buf = Buffer.alloc(1024);
  const n = GetWindowTextW(hwnd, buf, 512);
  return { hwnd: String(hwnd), pid: pid[0], title: buf.toString('utf16le', 0, n * 2), visible: IsWindowVisible(hwnd) };
}

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch { return false; }
}

const dir = path.join(os.homedir(), '.claude', 'sessions');
const sessions = fs.readdirSync(dir)
  .filter((f) => /^\d+\.json$/.test(f))
  .map((f) => { try { return JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8')); } catch { return null; } })
  .filter((s) => s && alive(s.pid));

const topLevel = [];
EnumWindows((hwnd) => {
  const d = describe(hwnd);
  if (d.visible && d.title) topLevel.push(d);
  return true;
}, 0);

FreeConsole();
const results = [];
for (const s of sessions) {
  const ok = AttachConsole(s.pid);
  const err = ok ? 0 : GetLastError();
  const consoleWnd = ok ? GetConsoleWindow() : null;
  const owner = consoleWnd ? GetWindow(consoleWnd, GW_OWNER) : null;
  const rootOwner = consoleWnd ? GetAncestor(consoleWnd, GA_ROOTOWNER) : null;
  if (ok) FreeConsole();
  results.push({
    pid: s.pid, sessionId: s.sessionId, name: s.name, status: s.status, cwd: s.cwd,
    attach: ok ? 'ok' : `failed (${err})`,
    consoleWindow: describe(consoleWnd), owner: describe(owner), rootOwner: describe(rootOwner),
  });
}

console.log(JSON.stringify({ sessions: results, terminalWindows: topLevel.filter((w) => /Terminal|Claude|PowerShell|cmd/i.test(w.title) || results.some((r) => r.owner?.pid === w.pid)) }, null, 2));
