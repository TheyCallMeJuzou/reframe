import { describe, expect, it } from 'vitest';
import { nextWaiting } from './attention';

describe('nextWaiting', () => {
  const order = ['a', 'b', 'c', 'd'];

  it('goes to the next waiting session after the current one, round to the start', () => {
    const waiting = new Set(['b', 'd']);
    expect(nextWaiting(order, waiting, null)).toBe('b');
    expect(nextWaiting(order, waiting, 'b')).toBe('d');
    expect(nextWaiting(order, waiting, 'c')).toBe('d');
    expect(nextWaiting(order, waiting, 'd')).toBe('b');
  });

  it('stays on the current one when it is the only one waiting, and knows when none is', () => {
    expect(nextWaiting(order, new Set(['c']), 'c')).toBe('c');
    expect(nextWaiting(order, new Set(), 'a')).toBeNull();
    expect(nextWaiting([], new Set(['a']), null)).toBeNull();
    // A current one that is gone counts as none.
    expect(nextWaiting(order, new Set(['a']), 'x')).toBe('a');
  });
});
