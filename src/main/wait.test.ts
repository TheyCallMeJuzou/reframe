import { afterEach, describe, expect, it, vi } from 'vitest';
import { settle, withTimeout } from './wait';

afterEach(() => vi.useRealTimers());

describe('withTimeout', () => {
  it('gives the value in time, else the fallback, also for a failure', async () => {
    vi.useFakeTimers();
    const late = withTimeout(new Promise<string>(() => {}), 1000, 'fallback');
    vi.advanceTimersByTime(1000);
    await expect(late).resolves.toBe('fallback');
    await expect(withTimeout(Promise.resolve('value'), 1000, 'fallback')).resolves.toBe('value');
    await expect(withTimeout(Promise.reject(new Error('no')), 1000, 'fallback')).resolves.toBe('fallback');
  });
});

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
