import { describe, expect, it } from 'vitest';
import { decodeWslOutput, parseDistroList, parseProbe } from './distros';

describe('decodeWslOutput and parseDistroList', () => {
  it('reads the list in UTF-16 (older wsl.exe) and UTF-8', () => {
    expect(parseDistroList(decodeWslOutput(Buffer.from('Ubuntu\r\nDebian\r\n', 'utf16le')))).toEqual([
      'Ubuntu',
      'Debian',
    ]);
    expect(parseDistroList(decodeWslOutput(Buffer.from('\uFEFFUbuntu-24.04\r\n\r\n', 'utf8')))).toEqual([
      'Ubuntu-24.04',
    ]);
    expect(parseDistroList('')).toEqual([]);
  });
});

describe('parseProbe', () => {
  const answer = (claude: string, configDir = '') =>
    [
      'Welcome to a chatty .bashrc',
      '',
      '@@REFRAME@@',
      '/home/ann/.local/bin:/usr/bin:/mnt/c/Users/Ann/AppData/Roaming/npm',
      claude,
      '/home/ann',
      configDir,
      'MOUNT=/mnt/c/',
    ].join('\r\n');

  it('finds claude, the PATH, the home and where the drives are, past what the shell prints', () => {
    expect(parseProbe('Ubuntu', answer('/home/ann/.local/bin/claude'))).toEqual({
      distro: 'Ubuntu',
      claude: '/home/ann/.local/bin/claude',
      path: '/home/ann/.local/bin:/usr/bin:/mnt/c/Users/Ann/AppData/Roaming/npm',
      home: '/home/ann',
      claudeHome: '/home/ann/.claude',
      mountRoot: '/mnt/',
    });
    expect(parseProbe('Ubuntu', answer('/usr/bin/claude', '/data/claude'))?.claudeHome).toBe('/data/claude');
  });

  it('does not count the Windows claude seen through the PATH, nor a missing one', () => {
    expect(parseProbe('Ubuntu', answer('/mnt/c/Users/Ann/AppData/Roaming/npm/claude'))?.claude).toBeNull();
    expect(parseProbe('Ubuntu', answer(''))?.claude).toBeNull();
  });

  it('gives up without an answer', () => {
    expect(parseProbe('Ubuntu', 'command not found')).toBeNull();
  });
});
