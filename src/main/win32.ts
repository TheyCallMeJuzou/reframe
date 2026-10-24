// Win32 calls the main process makes itself (no console attaching; see the console helper for that):
// following window drags of other apps and reading a foreign window. Loaded only on Windows.
import koffi from 'koffi';

export interface Win32 {
  /** Calls back when the user starts and ends moving a window of another app (title-bar drag). */
  onMoveSize(callback: (phase: 'start' | 'end', hwnd: string) => void): void;
  processId(hwnd: string): number;
  /** Executable file name of a process, e.g. "WindowsTerminal.exe". */
  imageName(pid: number): string | null;
  title(hwnd: string): string;
  /** Whether the window still exists. */
  isWindow(hwnd: string): boolean;
}

let instance: Win32 | null = null;

export function win32(): Win32 | null {
  if (process.platform !== 'win32') return null;
  instance ??= load();
  return instance;
}

function load(): Win32 {
  const user32 = koffi.load('user32.dll');
  const kernel32 = koffi.load('kernel32.dll');
  const WINEVENTPROC = koffi.proto(
    'void __stdcall WINEVENTPROC(void* hook, uint32_t event, void* hwnd, int32_t idObject, int32_t idChild, uint32_t thread, uint32_t time)',
  );
  const SetWinEventHook = user32.func(
    'void* __stdcall SetWinEventHook(uint32_t eventMin, uint32_t eventMax, void* module, WINEVENTPROC* callback, uint32_t pid, uint32_t thread, uint32_t flags)',
  );
  const GetWindowThreadProcessId = user32.func(
    'uint32_t __stdcall GetWindowThreadProcessId(void* hwnd, _Out_ uint32_t* pid)',
  );
  const GetWindowTextW = user32.func('int __stdcall GetWindowTextW(void* hwnd, _Out_ uint16_t* text, int max)');
  const IsWindow = user32.func('bool __stdcall IsWindow(void* hwnd)');
  const OpenProcess = kernel32.func('void* __stdcall OpenProcess(uint32_t access, bool inherit, uint32_t pid)');
  const QueryFullProcessImageNameW = kernel32.func(
    'bool __stdcall QueryFullProcessImageNameW(void* process, uint32_t flags, _Out_ uint16_t* name, _Inout_ uint32_t* size)',
  );
  const CloseHandle = kernel32.func('bool __stdcall CloseHandle(void* handle)');

  const EVENT_SYSTEM_MOVESIZESTART = 0x000a;
  const EVENT_SYSTEM_MOVESIZEEND = 0x000b;
  const WINEVENT_SKIPOWNPROCESS = 0x0002;
  const OBJID_WINDOW = 0;
  const PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;

  const toHandle = (hwnd: string) => BigInt(hwnd);
  const callbacks: unknown[] = []; // keeps registered callbacks alive

  return {
    onMoveSize(callback) {
      const registered = koffi.register((_hook: unknown, event: number, hwnd: bigint | null, idObject: number) => {
        if (idObject !== OBJID_WINDOW || !hwnd) return;
        callback(event === EVENT_SYSTEM_MOVESIZESTART ? 'start' : 'end', String(hwnd));
      }, koffi.pointer(WINEVENTPROC));
      callbacks.push(registered);
      // Out-of-context hooks are delivered through the message loop of this (the UI) thread.
      SetWinEventHook(
        EVENT_SYSTEM_MOVESIZESTART,
        EVENT_SYSTEM_MOVESIZEEND,
        null,
        registered,
        0,
        0,
        WINEVENT_SKIPOWNPROCESS,
      );
    },
    processId(hwnd) {
      const pid = [0];
      GetWindowThreadProcessId(toHandle(hwnd), pid);
      return pid[0]!;
    },
    imageName(pid) {
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
    },
    isWindow(hwnd) {
      return IsWindow(toHandle(hwnd));
    },
    title(hwnd) {
      const buffer = new Uint16Array(512);
      const length = GetWindowTextW(toHandle(hwnd), buffer, buffer.length);
      return String.fromCharCode(...buffer.slice(0, Math.max(0, length)));
    },
  };
}
