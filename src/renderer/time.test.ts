import { describe, expect, it } from 'vitest';
import { makeTranslate } from './context';
import { shortTime } from './time';

const now = new Date(2026, 10, 9, 15, 0).getTime();
const ago = (ms: number) => now - ms;
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

describe('shortTime', () => {
  const en = (time: number) => shortTime(time, now, makeTranslate('en'), 'en');
  const de = (time: number) => shortTime(time, now, makeTranslate('de'), 'de');

  it('counts minutes, hours and days for the last week', () => {
    expect(en(ago(20_000))).toBe('now');
    expect(en(ago(5 * MINUTE + 59_000))).toBe('5m');
    expect(en(ago(59 * MINUTE))).toBe('59m');
    expect(en(ago(2 * HOUR))).toBe('2h');
    expect(en(ago(3 * DAY))).toBe('3d');
    expect(de(ago(5 * MINUTE))).toBe('5 Min.');
    expect(de(ago(3 * DAY))).toBe('3 Tg.');
  });

  it('shows the date for older ones, with the year from another year', () => {
    expect(en(new Date(2026, 9, 28).getTime())).toBe('Oct 28');
    expect(de(new Date(2026, 9, 28).getTime())).toBe('28. Okt.');
    expect(en(new Date(2025, 10, 6).getTime())).toBe('Nov 2025');
  });

  it('takes a time a little ahead (another clock) as now', () => {
    expect(en(now + 5_000)).toBe('now');
  });
});
