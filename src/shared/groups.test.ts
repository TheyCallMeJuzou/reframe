import { describe, expect, it } from 'vitest';
import { GROUP_COLORS, isGroupColor, nextGroupColor, nextGroupNumber } from './groups';

describe('new group defaults', () => {
  it('takes the first free color, then repeats', () => {
    expect(nextGroupColor([])).toBe(GROUP_COLORS[0].hex);
    expect(nextGroupColor([GROUP_COLORS[0].hex, GROUP_COLORS[2].hex])).toBe(GROUP_COLORS[1].hex);
    const all = GROUP_COLORS.map((c) => c.hex);
    expect(nextGroupColor(all)).toBe(GROUP_COLORS[0].hex);
    expect(nextGroupColor([...all, all[0]!])).toBe(GROUP_COLORS[1].hex);
  });

  it('numbers groups by the lowest free name', () => {
    const name = (n: number) => `Group ${n}`;
    expect(nextGroupNumber([], name)).toBe(1);
    expect(nextGroupNumber(['Group 1', 'Group 3', 'API'], name)).toBe(2);
  });

  it('accepts only palette colors', () => {
    expect(isGroupColor(GROUP_COLORS[3].hex)).toBe(true);
    expect(isGroupColor('#123456')).toBe(false);
  });
});
