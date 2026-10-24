import { describe, expect, it } from 'vitest';
import { formatPaths, imagePasteKey } from './paste';

describe('formatPaths', () => {
  it('quotes Windows paths only where cmd and PowerShell would need it', () => {
    expect(formatPaths(['C:\\shots\\a.png', 'C:\\My Pictures\\b.png'], 'win32')).toBe(
      'C:\\shots\\a.png "C:\\My Pictures\\b.png" ',
    );
  });

  it('quotes other paths for a POSIX shell', () => {
    expect(formatPaths(['/home/me/a.png', "/home/me/it's here.png"], 'linux')).toBe(
      "/home/me/a.png '/home/me/it'\\''s here.png' ",
    );
  });

  it('types nothing for no files', () => {
    expect(formatPaths([], 'win32')).toBe('');
  });
});

describe('imagePasteKey', () => {
  it('is Alt+V on Windows and Ctrl+V elsewhere', () => {
    expect(imagePasteKey('win32')).toBe('\x1bv');
    expect(imagePasteKey('darwin')).toBe('\x16');
  });
});
