// Short-lived helper (Electron utilityProcess, Windows only): types Ctrl+C into another process's console,
// as if the user pressed it in that terminal. Attaching to a foreign console happens here so that the
// main process never switches consoles. Request: { pid, times, intervalMs }. Reply: { ok, error? }.
import koffi from 'koffi';

interface Request {
  pid: number;
  times: number;
  intervalMs: number;
}

const kernel32 = koffi.load('kernel32.dll');
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
const CreateFileW = kernel32.func(
  'void* __stdcall CreateFileW(str16 name, uint32_t access, uint32_t share, void* security, uint32_t disposition, uint32_t flags, void* template)',
);
const WriteConsoleInputW = kernel32.func(
  'bool __stdcall WriteConsoleInputW(void* handle, INPUT_RECORD* records, uint32_t count, _Out_ uint32_t* written)',
);
const CloseHandle = kernel32.func('bool __stdcall CloseHandle(void* handle)');
const GetLastError = kernel32.func('uint32_t __stdcall GetLastError()');

const GENERIC_READ_WRITE = 0xc0000000;
const FILE_SHARE_READ_WRITE = 3;
const OPEN_EXISTING = 3;
const INVALID_HANDLE = 0xffffffffffffffffn;
const KEY_EVENT = 1;
const LEFT_CTRL_PRESSED = 0x0008;

const ctrlC = (down: boolean) => ({
  EventType: KEY_EVENT,
  Event: {
    bKeyDown: down ? 1 : 0,
    wRepeatCount: 1,
    wVirtualKeyCode: 0x43, // C
    wVirtualScanCode: 0x2e,
    UnicodeChar: 3,
    dwControlKeyState: LEFT_CTRL_PRESSED,
  },
});

async function typeCtrlC({ pid, times, intervalMs }: Request): Promise<{ ok: boolean; error?: string }> {
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
    for (let i = 0; i < times; i++) {
      if (i > 0) await new Promise((resolve) => setTimeout(resolve, intervalMs));
      if (!WriteConsoleInputW(input, ctrlC(true), 1, written) || !WriteConsoleInputW(input, ctrlC(false), 1, written)) {
        return { ok: false, error: `WriteConsoleInput failed (${GetLastError()})` };
      }
    }
    return { ok: true };
  } finally {
    CloseHandle(input);
    FreeConsole();
  }
}

process.parentPort.once('message', (event) => {
  void typeCtrlC(event.data as Request)
    .catch((error: unknown) => ({ ok: false, error: String(error) }))
    .then((result) => {
      process.parentPort.postMessage(result);
      process.exit(0);
    });
});
