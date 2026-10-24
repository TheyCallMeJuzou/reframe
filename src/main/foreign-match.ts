// Pure decisions for taking over a terminal window that was dragged onto reframe.
import type { AttachedProcess, ConsoleOwner } from '../consolehelper/protocol';

/** Programs that host terminal windows; dragging any other window is ignored right away. */
const TERMINAL_HOSTS = new Set(
  [
    'WindowsTerminal.exe',
    'OpenConsole.exe',
    'conhost.exe',
    'cmd.exe',
    'powershell.exe',
    'pwsh.exe',
    'wezterm-gui.exe',
    'alacritty.exe',
    'mintty.exe',
    'ConEmu64.exe',
    'ConEmu.exe',
    'Tabby.exe',
    'Hyper.exe',
  ].map((n) => n.toLowerCase()),
);

/** Shells (and console hosts) that may be left in a terminal tab once claude has gone. */
const SHELLS = new Set(
  [
    'pwsh.exe',
    'powershell.exe',
    'cmd.exe',
    'bash.exe',
    'sh.exe',
    'zsh.exe',
    'fish.exe',
    'nu.exe',
    'wsl.exe',
    'conhost.exe',
    'OpenConsole.exe',
  ].map((n) => n.toLowerCase()),
);

export function isTerminalHost(imageName: string | null): boolean {
  return !!imageName && TERMINAL_HOSTS.has(imageName.toLowerCase());
}

export function isShell(imageName: string | null): boolean {
  return !!imageName && SHELLS.has(imageName.toLowerCase());
}

/**
 * Pids whose console lives in window `hwnd`. Windows Terminal makes the window that hosts a tab the owner of
 * the tab's (hidden) pseudo-console window; a classic console window is the console window itself.
 */
export function pidsInWindow(hwnd: string, owners: ConsoleOwner[]): number[] {
  return owners.filter((o) => o.consoleWindow === hwnd || o.owner === hwnd || o.rootOwner === hwnd).map((o) => o.pid);
}

/** Whether nothing but shells runs in a console (an empty list means the console is already gone). */
export function onlyShells(processes: AttachedProcess[]): boolean {
  return processes.every((p) => isShell(p.name));
}

/** The session the window shows right now, judged by its title (Windows Terminal shows the active tab's). */
export function activeByTitle<T extends { name?: string }>(title: string, candidates: T[]): T | undefined {
  return candidates
    .filter((c) => c.name && title.includes(c.name))
    .sort((a, b) => (b.name?.length ?? 0) - (a.name?.length ?? 0))[0];
}
