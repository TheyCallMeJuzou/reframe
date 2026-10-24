import { describe, expect, it } from 'vitest';
import { fileUrls, plistStrings } from './clipboard-formats';

describe('copied files', () => {
  it('reads the file names macOS puts on the clipboard', () => {
    const plist =
      '<?xml version="1.0"?><plist version="1.0"><array><string>/Users/me/a.png</string>' +
      '<string>/Users/me/Tom &amp; Jerry.txt</string></array></plist>';
    expect(plistStrings(plist)).toEqual(['/Users/me/a.png', '/Users/me/Tom & Jerry.txt']);
  });

  it('reads file URLs, also GNOME lists that start with "copy"', () => {
    if (process.platform === 'win32') {
      expect(fileUrls('file:///C:/shots/a%20b.png\r\n')).toEqual(['C:\\shots\\a b.png']);
      return;
    }
    expect(fileUrls('copy\nfile:///home/me/a%20b.png\nfile:///tmp/c.txt')).toEqual(['/home/me/a b.png', '/tmp/c.txt']);
  });
});
