import { describe, expect, it } from 'vitest';
import { processStart } from './process';

describe('processStart', () => {
  it.runIf(process.platform === 'win32')('reads when a process was created, as Claude Code writes it', () => {
    const start = processStart(process.pid);
    expect(start).toMatch(/^\d{17,}$/);
    expect(processStart(process.pid)).toBe(start);
    expect(processStart(999_999_999)).toBeNull();
  });

  it.skipIf(process.platform === 'win32')('cannot tell elsewhere', () => {
    expect(processStart(process.pid)).toBeNull();
  });
});
