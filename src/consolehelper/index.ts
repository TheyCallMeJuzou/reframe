// Short-lived helper (Electron utilityProcess, Windows only) for things that need to attach to another
// process's console: typing Ctrl+C into it, finding which terminal window hosts it, and listing what runs
// in it. Attaching happens here so that the main process never switches consoles. One request per process.
import koffi from 'koffi';
import type { AttachedProcess, ConsoleOwner, ConsoleRequest, ConsoleResponse } from './protocol';

const kernel32 = koffi.load('kernel32.dll');
const user32 = koffi.load('user32.dll');
const KEY_EVENT_RECORD = koffi.struct('KEY_EVENT_RECORD', {
  bKeyDown: 'int32',
  wRepeatCount: 'uint16',
  wVirtualKeyCode: 'uint16',
  wVirtualScanCode: 'uint16',
  UnicodeChar: 'uint16',
  dwControlKeyState: 'uint32',
});
// Registered by name for the prototypes below.
koffi.struct('INPUT_RECORD', { EventType: 'uint16', Event: KEY_EVENT_RECORD });
const FreeConsole = kernel32.func('bool __stdcall FreeConsole()');
const AttachConsole = kernel32.func('bool __stdcall AttachConsole(uint32_t pid)');
const GetConsoleWindow = kernel32.func('void* __stdcall GetConsoleWindow()');
const GetConsoleProcessList = kernel32.func(
  'uint32_t __stdcall GetConsoleProcessList(_Out_ uint32_t* list, uint32_t count)',
);
const CreateFileW = kernel32.func(
  'void* __stdcall CreateFileW(str16 name, uint32_t access, uint32_t share, void* security, uint32_t disposition, uint32_t flags, void* template)',
);
const WriteConsoleInputW = kernel32.func(
  'bool __stdcall WriteConsoleInputW(void* handle, INPUT_RECORD* records, uint32_t count, _Out_ uint32_t* written)',
);
const OpenProcess = kernel32.func('void* __stdcall OpenProcess(uint32_t access, bool inherit, uint32_t pid)');
const QueryFullProcessImageNameW = kernel32.func(
  'bool __stdcall QueryFullProcessImageNameW(void* process, uint32_t flags, _Out_ uint16_t* name, _Inout_ uint32_t* size)',
);
const CloseHandle = kernel32.func('bool __stdcall CloseHandle(void* handle)');
const GetLastError = kernel32.func('uint32_t __stdcall GetLastError()');
const GetWindow = user32.func('void* __stdcall GetWindow(void* window, uint32_t command)');
const GetAncestor = user32.func('void* __stdcall GetAncestor(void* window, uint32_t flags)');
const VkKeyScanW = user32.func('int16_t __stdcall VkKeyScanW(uint16_t ch)');
const MapVirtualKeyW = user32.func('uint32_t __stdcall MapVirtualKeyW(uint32_t code, uint32_t mapType)');

const GENERIC_READ_WRITE = 0xc0000000;
const FILE_SHARE_READ_WRITE = 3;
const OPEN_EXISTING = 3;
const INVALID_HANDLE = 0xffffffffffffffffn;
const KEY_EVENT = 1;
const LEFT_CTRL_PRESSED = 0x0008;
const SHIFT_PRESSED = 0x0010;
const GW_OWNER = 4;
const GA_ROOTOWNER = 3;
const PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;

const handle = (value: unknown): string | null => (value ? String(value) : null);

interface Key {
  vk: number;
  scan: number;
  char: number;
  state: number;
}

const CTRL_C: Key = { vk: 0x43, scan: 0x2e, char: 3, state: LEFT_CTRL_PRESSED };

/** The key that produces `ch` on the current keyboard layout ("\r" is Enter). */
function keyFor(ch: string): Key {
  if (ch === '\r') return { vk: 0x0d, scan: 0x1c, char: 13, state: 0 };
  const code = VkKeyScanW(ch.charCodeAt(0));
  const vk = code & 0xff;
  return { vk, scan: MapVirtualKeyW(vk, 0), char: ch.charCodeAt(0), state: (code >> 8) & 1 ? SHIFT_PRESSED : 0 };
}

const record = (key: Key, down: boolean) => ({
  EventType: KEY_EVENT,
  Event: {
    bKeyDown: down ? 1 : 0,
    wRepeatCount: 1,
    wVirtualKeyCode: key.vk,
    wVirtualScanCode: key.scan,
    UnicodeChar: key.char,
    dwControlKeyState: key.state,
  },
});

/** Runs `use` while attached to the console of `pid`; always detaches again. */
function withConsole<T>(pid: number, use: () => T): T | Error {
  FreeConsole();
  if (!AttachConsole(pid)) return new Error(`AttachConsole(${pid}) failed (${GetLastError()})`);
  try {
    return use();
  } finally {
    FreeConsole();
  }
}

/** Types groups of keys into the console of `pid`, as if pressed there, with a pause between groups. */
async function typeKeys(pid: number, groups: Key[][], pauseMs: number): Promise<ConsoleResponse> {
  FreeConsole();
  if (!AttachConsole(pid)) return { ok: false, error: `AttachConsole failed (${GetLastError()})` };
  const input = CreateFileW('CONIN$', GENERIC_READ_WRITE, FILE_SHARE_READ_WRITE, null, OPEN_EXISTING, 0, null);
  if (!input || input === INVALID_HANDLE) {
    const error = `CONIN$ not available (${GetLastError()})`;
    FreeConsole();
    return { ok: false, error };
  }
  try {
    const written = [0];
    for (const [i, keys] of groups.entries()) {
      if (i > 0) await new Promise((resolve) => setTimeout(resolve, pauseMs));
      for (const key of keys) {
        if (
          !WriteConsoleInputW(input, record(key, true), 1, written) ||
          !WriteConsoleInputW(input, record(key, false), 1, written)
        ) {
          return { ok: false, error: `WriteConsoleInput failed (${GetLastError()})` };
        }
      }
    }
    return { ok: true };
  } finally {
    CloseHandle(input);
    FreeConsole();
  }
}

function owners(pids: number[]): ConsoleResponse {
  const result: ConsoleOwner[] = pids.map((pid) => {
    const found = withConsole(pid, () => {
      const consoleWindow = GetConsoleWindow();
      return {
        pid,
        consoleWindow: handle(consoleWindow),
        owner: consoleWindow ? handle(GetWindow(consoleWindow, GW_OWNER)) : null,
        rootOwner: consoleWindow ? handle(GetAncestor(consoleWindow, GA_ROOTOWNER)) : null,
      };
    });
    return found instanceof Error ? { pid, consoleWindow: null, owner: null, rootOwner: null } : found;
  });
  return { ok: true, owners: result };
}

function consoleProcesses(pid: number): ConsoleResponse {
  const pids = withConsole(pid, () => {
    const list = new Uint32Array(256);
    const count = GetConsoleProcessList(list, list.length);
    return Array.from(list.slice(0, Math.min(count, list.length)));
  });
  if (pids instanceof Error) return { ok: false, error: pids.message };
  const processes: AttachedProcess[] = pids
    .filter((p) => p !== process.pid)
    .map((p) => ({ pid: p, name: imageName(p) }));
  return { ok: true, processes };
}

function imageName(pid: number): string | null {
  const target = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
  if (!target) return null;
  try {
    const buffer = new Uint16Array(1024);
    const size = [buffer.length];
    if (!QueryFullProcessImageNameW(target, 0, buffer, size)) return null;
    const full = String.fromCharCode(...buffer.slice(0, size[0]));
    return full.split('\\').pop() ?? full;
  } finally {
    CloseHandle(target);
  }
}

async function handleRequest(request: ConsoleRequest): Promise<ConsoleResponse> {
  switch (request.type) {
    case 'interrupt':
      return typeKeys(
        request.pid,
        Array.from({ length: request.times }, () => [CTRL_C]),
        request.intervalMs,
      );
    case 'type':
      return typeKeys(request.pid, [[...request.text].map(keyFor)], 0);
    case 'owners':
      return owners(request.pids);
    case 'console-processes':
      return consoleProcesses(request.pid);
  }
}

process.parentPort.once('message', (event) => {
  void handleRequest(event.data as ConsoleRequest)
    .catch((error: unknown): ConsoleResponse => ({ ok: false, error: String(error) }))
    .then((result) => {
      process.parentPort.postMessage(result);
      process.exit(0);
    });
});
