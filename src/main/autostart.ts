// Starting reframe with the system. Windows and macOS keep it among their login items; Linux desktops start
// what is in ~/.config/autostart. reframe then starts in the tray, with its windows hidden and every session
// resumed in the background.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { app } from 'electron';

/** Marks a start by the system (Windows and Linux; macOS tells it through its login item settings). */
export const AUTOSTART_ARG = '--autostart';

/** The program and arguments that start this reframe; in development, Electron needs the app's folder. */
export function launchCommand(
  info: { execPath: string; appPath: string; packaged: boolean; appImage?: string } = {
    execPath: process.execPath,
    appPath: app.getAppPath(),
    packaged: app.isPackaged,
    appImage: process.env.APPIMAGE,
  },
): { file: string; args: string[] } {
  const file = info.appImage ?? info.execPath;
  return { file, args: info.packaged ? [AUTOSTART_ARG] : [info.appPath, AUTOSTART_ARG] };
}

/** An XDG autostart entry for that command. */
export function desktopEntry(command: { file: string; args: string[] }): string {
  const quote = (value: string) => `"${value.replace(/(["`$\\])/g, '\\$1')}"`;
  return [
    '[Desktop Entry]',
    'Type=Application',
    'Name=reframe',
    'Comment=Claude Code sessions side by side',
    `Exec=${[command.file, ...command.args].map(quote).join(' ')}`,
    'X-GNOME-Autostart-enabled=true',
    '',
  ].join('\n');
}

const linuxEntry = () =>
  path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'autostart', 'reframe.desktop');

export function autostartEnabled(): boolean {
  if (process.platform === 'linux') return fs.existsSync(linuxEntry());
  const { file, args } = launchCommand();
  return app.getLoginItemSettings({ path: file, args }).openAtLogin;
}

export function setAutostart(enabled: boolean): void {
  const command = launchCommand();
  if (process.platform === 'linux') {
    const entry = linuxEntry();
    if (!enabled) return fs.rmSync(entry, { force: true });
    fs.mkdirSync(path.dirname(entry), { recursive: true });
    fs.writeFileSync(entry, desktopEntry(command));
    return;
  }
  app.setLoginItemSettings({ openAtLogin: enabled, path: command.file, args: command.args });
}

/** Whether the system started reframe (then it stays in the tray). */
export function startedBySystem(): boolean {
  if (process.argv.includes(AUTOSTART_ARG)) return true;
  return process.platform === 'darwin' && app.getLoginItemSettings().wasOpenedAtLogin;
}
