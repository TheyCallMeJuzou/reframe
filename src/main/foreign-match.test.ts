import { describe, expect, it } from 'vitest';
import { activeByTitle, isTerminalHost, onlyShells, pidsInWindow } from './foreign-match';

describe('foreign terminal windows', () => {
  it('recognizes terminal hosts regardless of case', () => {
    expect(isTerminalHost('WindowsTerminal.exe')).toBe(true);
    expect(isTerminalHost('windowsterminal.EXE')).toBe(true);
    expect(isTerminalHost('chrome.exe')).toBe(false);
    expect(isTerminalHost(null)).toBe(false);
  });

  it('maps consoles to the window hosting them', () => {
    const owners = [
      { pid: 1, consoleWindow: '100', owner: '500', rootOwner: '500' }, // tab in Windows Terminal window 500
      { pid: 2, consoleWindow: '200', owner: '600', rootOwner: '600' }, // tab in another window
      { pid: 3, consoleWindow: '700', owner: null, rootOwner: '700' }, // classic console window 700
      { pid: 4, consoleWindow: null, owner: null, rootOwner: null }, // could not attach
    ];
    expect(pidsInWindow('500', owners)).toEqual([1]);
    expect(pidsInWindow('700', owners)).toEqual([3]);
    expect(pidsInWindow('999', owners)).toEqual([]);
  });

  it('closes only consoles with nothing but shells left', () => {
    expect(
      onlyShells([
        { pid: 1, name: 'pwsh.exe' },
        { pid: 2, name: 'OpenConsole.exe' },
      ]),
    ).toBe(true);
    expect(
      onlyShells([
        { pid: 1, name: 'pwsh.exe' },
        { pid: 2, name: 'node.exe' },
      ]),
    ).toBe(false);
    expect(onlyShells([{ pid: 1, name: null }])).toBe(false);
    expect(onlyShells([])).toBe(true);
  });

  it('picks the session whose name the window title shows', () => {
    const candidates = [{ name: 'api' }, { name: 'api-docs' }, {}];
    expect(activeByTitle('✳ api-docs', candidates)).toEqual({ name: 'api-docs' });
    expect(activeByTitle('✳ Claude Code', candidates)).toBeUndefined();
  });
});
