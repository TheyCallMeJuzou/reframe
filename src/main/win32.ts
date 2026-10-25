// Win32 calls the main process makes itself (no console attaching; see the console helper for that):
// following window drags and new windows of other apps, and reading a foreign window. Loaded only on Windows.
import koffi from 'koffi';

export interface Win32 {
  /** Calls back when the user starts and ends moving a window of another app (title-bar drag). */
  onMoveSize(callback: (phase: 'start' | 'end', hwnd: string) => void): void;
  /** Calls back when another app creates or shows a top-level window. */
  onTopLevel(callback: (phase: 'created' | 'shown', hwnd: string) => void): void;
  /** Window rectangle in physical screen pixels. */
  rect(hwnd: string): { x: number; y: number; width: number; height: number } | null;
  /** The window in front (the one the user works in), if any. */
  foreground(): string | null;
  processId(hwnd: string): number;
  /** Executable file name of a process, e.g. "WindowsTerminal.exe". */
  imageName(pid: number): string | null;
  title(hwnd: string): string;
  /** Whether the window still exists. */
  isWindow(hwnd: string): boolean;
  /** Files copied in Explorer (or another file manager) that are on the clipboard. */
  clipboardFiles(): string[];
  /** Has Windows close reframe before other programs (its claudes among them) when it shuts down or logs off. */
  shutDownFirst(): void;
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
  const GetForegroundWindow = user32.func('void* __stdcall GetForegroundWindow()');
  const GetAncestor = user32.func('void* __stdcall GetAncestor(void* hwnd, uint32_t flags)');
  koffi.struct('RECT', { left: 'int32_t', top: 'int32_t', right: 'int32_t', bottom: 'int32_t' });
  const GetWindowRect = user32.func('bool __stdcall GetWindowRect(void* hwnd, _Out_ RECT* rect)');
  const OpenProcess = kernel32.func('void* __stdcall OpenProcess(uint32_t access, bool inherit, uint32_t pid)');
  const QueryFullProcessImageNameW = kernel32.func(
    'bool __stdcall QueryFullProcessImageNameW(void* process, uint32_t flags, _Out_ uint16_t* name, _Inout_ uint32_t* size)',
  );
  const CloseHandle = kernel32.func('bool __stdcall CloseHandle(void* handle)');
  const SetProcessShutdownParameters = kernel32.func(
    'bool __stdcall SetProcessShutdownParameters(uint32_t level, uint32_t flags)',
  );
  const shell32 = koffi.load('shell32.dll');
  const IsClipboardFormatAvailable = user32.func('bool __stdcall IsClipboardFormatAvailable(uint32_t format)');
  const OpenClipboard = user32.func('bool __stdcall OpenClipboard(void* owner)');
  const CloseClipboard = user32.func('bool __stdcall CloseClipboard()');
  const GetClipboardData = user32.func('void* __stdcall GetClipboardData(uint32_t format)');
  const DragQueryFileW = shell32.func(
    'uint32_t __stdcall DragQueryFileW(void* drop, uint32_t index, _Out_ uint16_t* file, uint32_t size)',
  );

  const EVENT_SYSTEM_MOVESIZESTART = 0x000a;
  const EVENT_SYSTEM_MOVESIZEEND = 0x000b;
  const EVENT_OBJECT_CREATE = 0x8000;
  const EVENT_OBJECT_SHOW = 0x8002;
  const CHILDID_SELF = 0;
  const GA_ROOT = 2;
  const WINEVENT_SKIPOWNPROCESS = 0x0002;
  const OBJID_WINDOW = 0;
  const PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
  const CF_HDROP = 15;
  // Programs start at 0x280; 0x300 to 0x3FF is the range of programs Windows closes first.
  const SHUTDOWN_LEVEL_FIRST = 0x300;

  const toHandle = (hwnd: string) => BigInt(hwnd);
  const callbacks: unknown[] = []; // keeps registered callbacks alive
  const hook = (eventMin: number, eventMax: number, callback: (...args: never[]) => void) => {
    const registered = koffi.register(callback, koffi.pointer(WINEVENTPROC));
    callbacks.push(registered);
    // Out-of-context hooks are delivered through the message loop of this (the UI) thread.
    SetWinEventHook(eventMin, eventMax, null, registered, 0, 0, WINEVENT_SKIPOWNPROCESS);
  };

  return {
    onMoveSize(callback) {
      hook(
        EVENT_SYSTEM_MOVESIZESTART,
        EVENT_SYSTEM_MOVESIZEEND,
        (_hook: unknown, event: number, hwnd: bigint | null, idObject: number) => {
          if (idObject !== OBJID_WINDOW || !hwnd) return;
          callback(event === EVENT_SYSTEM_MOVESIZESTART ? 'start' : 'end', String(hwnd));
        },
      );
    },
    onTopLevel(callback) {
      // The range includes EVENT_OBJECT_DESTROY, which is skipped like every other event.
      hook(
        EVENT_OBJECT_CREATE,
        EVENT_OBJECT_SHOW,
        (_hook: unknown, event: number, hwnd: bigint | null, idObject: number, idChild: number) => {
          if (idObject !== OBJID_WINDOW || idChild !== CHILDID_SELF || !hwnd) return;
          if (event !== EVENT_OBJECT_CREATE && event !== EVENT_OBJECT_SHOW) return;
          if (String(GetAncestor(hwnd, GA_ROOT)) !== String(hwnd)) return; // a child window
          callback(event === EVENT_OBJECT_CREATE ? 'created' : 'shown', String(hwnd));
        },
      );
    },
    rect(hwnd) {
      const rect = { left: 0, top: 0, right: 0, bottom: 0 };
      if (!GetWindowRect(toHandle(hwnd), rect)) return null;
      return { x: rect.left, y: rect.top, width: rect.right - rect.left, height: rect.bottom - rect.top };
    },
    foreground() {
      const hwnd: unknown = GetForegroundWindow();
      return hwnd ? String(hwnd) : null;
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
    shutDownFirst() {
      SetProcessShutdownParameters(SHUTDOWN_LEVEL_FIRST, 0);
    },
    clipboardFiles() {
      if (!IsClipboardFormatAvailable(CF_HDROP) || !OpenClipboard(null)) return [];
      try {
        const drop: unknown = GetClipboardData(CF_HDROP);
        if (!drop) return [];
        const files: string[] = [];
        const count = DragQueryFileW(drop, 0xffffffff, null, 0) as number;
        for (let i = 0; i < count; i++) {
          const length = DragQueryFileW(drop, i, null, 0) as number;
          const buffer = new Uint16Array(length + 1);
          DragQueryFileW(drop, i, buffer, buffer.length);
          files.push(String.fromCharCode(...buffer.slice(0, length)));
        }
        return files;
      } finally {
        CloseClipboard();
      }
    },
    title(hwnd) {
      const buffer = new Uint16Array(512);
      const length = GetWindowTextW(toHandle(hwnd), buffer, buffer.length);
      return String.fromCharCode(...buffer.slice(0, Math.max(0, length)));
    },
  };
}
