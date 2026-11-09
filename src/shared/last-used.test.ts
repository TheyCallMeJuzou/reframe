import { describe, expect, it } from 'vitest';
import { countsAsUse } from './last-used';

describe('countsAsUse', () => {
  it('counts a prompt, Claude done, and Claude asking', () => {
    expect(countsAsUse('idle', 'busy')).toBe(true);
    expect(countsAsUse('busy', 'idle')).toBe(true);
    expect(countsAsUse('busy', 'waiting')).toBe(true);
    expect(countsAsUse('idle', 'waiting')).toBe(true);
    expect(countsAsUse('waiting', 'busy')).toBe(true);
  });

  it('does not count starting, resuming or ending one', () => {
    expect(countsAsUse('starting', 'idle')).toBe(false);
    expect(countsAsUse('idle', 'ended')).toBe(false);
    expect(countsAsUse('idle', 'error')).toBe(false);
    expect(countsAsUse('waiting', 'idle')).toBe(false);
    expect(countsAsUse('busy', 'busy')).toBe(false);
  });
});
