import { describe, expect, it } from 'vitest';
import { linuxPathOf, parseWslPath, wslWindowsPath } from './wsl';

describe('parseWslPath', () => {
  it('knows both names of the WSL share, in any case and with either slash', () => {
    expect(parseWslPath('\\\\wsl.localhost\\Ubuntu\\home\\ann\\shop')).toEqual({
      distro: 'Ubuntu',
      path: '/home/ann/shop',
    });
    expect(parseWslPath('\\\\WSL$\\Debian\\srv\\')).toEqual({ distro: 'Debian', path: '/srv' });
    expect(parseWslPath('//wsl.localhost/Ubuntu')).toEqual({ distro: 'Ubuntu', path: '/' });
  });

  it('takes a share of its own (tests) and leaves other folders alone', () => {
    expect(parseWslPath('C:\\tmp\\share\\Fake\\home\\ann', 'C:\\tmp\\share')).toEqual({
      distro: 'Fake',
      path: '/home/ann',
    });
    expect(parseWslPath('C:\\Users\\ann\\shop')).toBeNull();
    expect(parseWslPath('\\\\server\\share\\shop')).toBeNull();
    expect(parseWslPath('\\\\wsl.localhost\\')).toBeNull();
    expect(parseWslPath('\\\\wsl.localhostX\\Ubuntu\\home')).toBeNull();
  });
});

describe('wslWindowsPath', () => {
  it('puts a Linux path under the share', () => {
    expect(wslWindowsPath('Ubuntu', '/home/ann/shop', '\\\\wsl.localhost')).toBe(
      '\\\\wsl.localhost\\Ubuntu\\home\\ann\\shop',
    );
    expect(wslWindowsPath('Ubuntu', '/', 'C:\\tmp\\share\\')).toBe('C:\\tmp\\share\\Ubuntu');
  });
});

describe('linuxPathOf', () => {
  it('reaches drives under the mount root and the distribution folders as Linux paths', () => {
    expect(linuxPathOf('C:\\Users\\Ann\\Pictures\\shot.png', 'Ubuntu', '/mnt/')).toBe(
      '/mnt/c/Users/Ann/Pictures/shot.png',
    );
    expect(linuxPathOf('D:\\', 'Ubuntu', '/mnt')).toBe('/mnt/d');
    expect(linuxPathOf('\\\\wsl.localhost\\Ubuntu\\home\\ann\\a b.txt', 'ubuntu', '/mnt/')).toBe('/home/ann/a b.txt');
  });

  it('has no path for what the distribution cannot reach', () => {
    expect(linuxPathOf('\\\\wsl.localhost\\Debian\\home\\x', 'Ubuntu', '/mnt/')).toBeNull();
    expect(linuxPathOf('\\\\server\\share\\x', 'Ubuntu', '/mnt/')).toBeNull();
  });
});
