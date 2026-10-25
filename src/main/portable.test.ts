import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { portableDataDir } from './portable';

const temps: string[] = [];
function folder(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-portable-'));
  temps.push(dir);
  return dir;
}
afterEach(() => {
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe('portableDataDir', () => {
  it('is the folder "data" next to reframe.exe, when there is one', () => {
    const app = folder();
    const exe = path.join(app, 'reframe.exe');
    expect(portableDataDir(exe, 'win32')).toBeNull();
    fs.writeFileSync(path.join(app, 'data'), 'not a folder');
    expect(portableDataDir(exe, 'win32')).toBeNull();
    fs.rmSync(path.join(app, 'data'));
    fs.mkdirSync(path.join(app, 'data'));
    expect(portableDataDir(exe, 'win32')).toBe(path.win32.join(app, 'data'));
  });

  it('is only a thing on Windows', () => {
    const app = folder();
    fs.mkdirSync(path.join(app, 'data'));
    expect(portableDataDir(path.join(app, 'reframe'), 'linux')).toBeNull();
  });
});
