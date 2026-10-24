import { describe, expect, it } from 'vitest';
import { shortcutFor, shortcutLabel } from './shortcuts';

const press = (code: string, mods: { ctrl?: boolean; meta?: boolean; shift?: boolean; alt?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  metaKey: !!mods.meta,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

describe('shortcutFor', () => {
  it('knows the Ctrl+Shift shortcuts and Ctrl+1…9', () => {
    expect(shortcutFor(press('KeyT', { ctrl: true, shift: true }), false)).toEqual({ kind: 'newSession' });
    expect(shortcutFor(press('KeyN', { ctrl: true, shift: true }), false)).toEqual({ kind: 'newWindow' });
    expect(shortcutFor(press('KeyM', { ctrl: true, shift: true }), false)).toEqual({ kind: 'toggleCollapse' });
    expect(shortcutFor(press('KeyL', { ctrl: true, shift: true }), false)).toEqual({ kind: 'toggleSidebar' });
    expect(shortcutFor(press('ArrowLeft', { ctrl: true, shift: true }), false)).toEqual({
      kind: 'focusDirection',
      direction: 'left',
    });
    expect(shortcutFor(press('Digit3', { ctrl: true }), false)).toEqual({ kind: 'focusTile', index: 2 });
  });

  it('leaves everything else to the terminal (Claude Code uses plain Ctrl keys)', () => {
    expect(shortcutFor(press('KeyT', { ctrl: true }), false)).toBeNull();
    expect(shortcutFor(press('KeyV', { ctrl: true, shift: true }), false)).toBeNull();
    expect(shortcutFor(press('KeyT', { ctrl: true, shift: true, alt: true }), false)).toBeNull();
    expect(shortcutFor(press('Digit0', { ctrl: true }), false)).toBeNull();
    expect(shortcutFor(press('KeyT', { shift: true }), false)).toBeNull();
  });

  it('uses Cmd instead of Ctrl on macOS', () => {
    expect(shortcutFor(press('KeyT', { meta: true, shift: true }), true)).toEqual({ kind: 'newSession' });
    expect(shortcutFor(press('KeyT', { ctrl: true, shift: true }), true)).toBeNull();
    expect(shortcutFor(press('Digit1', { meta: true }), false)).toBeNull();
  });
});

describe('shortcutLabel', () => {
  it('writes shortcuts the platform way', () => {
    expect(shortcutLabel('T', { ctrl: 'Strg', shift: 'Umschalt' }, false)).toBe('Strg+Umschalt+T');
    expect(shortcutLabel('1…9', { ctrl: 'Ctrl', shift: 'Shift' }, false, false)).toBe('Ctrl+1…9');
    expect(shortcutLabel('T', { ctrl: 'Ctrl', shift: 'Shift' }, true)).toBe('⌘⇧T');
  });
});
