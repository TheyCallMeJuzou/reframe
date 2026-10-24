import { describe, expect, it } from 'vitest';
import { homeRelative } from './paths';

describe('homeRelative', () => {
  it('shows folders under the home folder from "~"', () => {
    expect(homeRelative('C:\\Users\\Ann\\projects\\shop', 'C:\\Users\\Ann', true)).toBe('~\\projects\\shop');
    expect(homeRelative('c:\\users\\ann\\projects', 'C:\\Users\\Ann\\', true)).toBe('~\\projects');
    expect(homeRelative('/home/ann/projects/shop', '/home/ann', false)).toBe('~/projects/shop');
    expect(homeRelative('/home/ann', '/home/ann', false)).toBe('~');
  });

  it('leaves other folders alone, also ones whose name only starts like the home folder', () => {
    expect(homeRelative('D:\\work\\shop', 'C:\\Users\\Ann', true)).toBe('D:\\work\\shop');
    expect(homeRelative('/home/anna/shop', '/home/ann', false)).toBe('/home/anna/shop');
    expect(homeRelative('/home/Ann/shop', '/home/ann', false)).toBe('/home/Ann/shop'); // case matters there
    expect(homeRelative('/srv/shop', '', false)).toBe('/srv/shop');
  });
});
