import { describe, expect, it } from 'vitest';
import { AUTOSTART_ARG, desktopEntry, launchCommand } from './autostart';

describe('autostart', () => {
  it('starts the installed app, or Electron with the app folder in development', () => {
    const installed = { execPath: 'C:\\Apps\\reframe\\reframe.exe', appPath: 'unused', packaged: true };
    expect(launchCommand(installed)).toEqual({ file: 'C:\\Apps\\reframe\\reframe.exe', args: [AUTOSTART_ARG] });
    const dev = { execPath: '/repo/node_modules/electron/dist/electron', appPath: '/repo', packaged: false };
    expect(launchCommand(dev)).toEqual({ file: dev.execPath, args: ['/repo', AUTOSTART_ARG] });
  });

  it('starts an AppImage from where the AppImage file is, not from its temporary mount', () => {
    const info = {
      execPath: '/tmp/.mount_reframe/reframe',
      appPath: '',
      packaged: true,
      appImage: '/home/me/reframe.AppImage',
    };
    expect(launchCommand(info).file).toBe('/home/me/reframe.AppImage');
  });

  it('writes a desktop entry with a quoted command line', () => {
    const entry = desktopEntry({ file: '/home/me/My Apps/reframe.AppImage', args: [AUTOSTART_ARG] });
    expect(entry).toContain('Exec="/home/me/My Apps/reframe.AppImage" "--autostart"\n');
    expect(entry).toContain('[Desktop Entry]\nType=Application\n');
    expect(desktopEntry({ file: '/a/"b"$c', args: [] })).toContain('Exec="/a/\\"b\\"\\$c"');
  });
});
