import { afterEach, describe, expect, it, vi } from 'vitest';
import { settle } from './wait';

afterEach(() => vi.useRealTimers());

describe('settle', () => {
  it('tells whether a promise settles in time, rejected ones too', async () => {
    vi.useFakeTimers();
    const never = settle(new Promise(() => {}), 1000);
    vi.advanceTimersByTime(1000);
    await expect(never).resolves.toBe(false);
    await expect(settle(Promise.resolve(), 1000)).resolves.toBe(true);
    await expect(settle(Promise.reject(new Error('no')), 1000)).resolves.toBe(true);
  });
});
