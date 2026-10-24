import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createLogger } from './log';

const temps: string[] = [];
afterEach(() => {
  for (const dir of temps.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe('createLogger', () => {
  it('appends one line per event with level and data', () => {
    const dir = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-log-')), 'logs');
    temps.push(path.dirname(dir));
    const log = createLogger(dir);
    log('info', 'session started', { id: 'a' });
    log('info', 'second');
    const lines = fs.readFileSync(log.file, 'utf8').trim().split('\n');
    expect(lines).toHaveLength(2);
    expect(lines[0]).toMatch(/^\d{4}-\d\d-\d\dT.* INFO {2}session started {"id":"a"}$/);
  });

  it('rotates a large file', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'reframe-log-'));
    temps.push(dir);
    fs.writeFileSync(path.join(dir, 'main.log'), 'x'.repeat(2 * 1024 * 1024 + 1));
    createLogger(dir)('info', 'fresh');
    expect(fs.readFileSync(path.join(dir, 'main.log'), 'utf8')).toContain('fresh');
    expect(fs.statSync(path.join(dir, 'main.old.log')).size).toBeGreaterThan(2 * 1024 * 1024);
  });
});
