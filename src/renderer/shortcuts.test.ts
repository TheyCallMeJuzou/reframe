import { describe, expect, it } from 'vitest';
import { fontSizeAfter, shortcutFor, shortcutLabel } from './shortcuts';

/** A key press; `key` is the character it makes (by default the letter or digit of `code`). */
const press = (
  code: string,
  mods: { ctrl?: boolean; meta?: boolean; shift?: boolean; alt?: boolean } = {},
  key = code.replace(/^(Key|Digit)/, '').toLowerCase(),
) => ({
  code,
  key,
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
    expect(shortcutFor(press('F11'), false)).toEqual({ kind: 'toggleFullscreen' });
    expect(shortcutFor(press('F11', { shift: true }), false)).toBeNull();
  });

  it('leaves everything else to the terminal (Claude Code uses plain Ctrl keys)', () => {
    expect(shortcutFor(press('KeyT', { ctrl: true }), false)).toBeNull();
    expect(shortcutFor(press('KeyV', { ctrl: true, shift: true }), false)).toBeNull();
    expect(shortcutFor(press('KeyT', { ctrl: true, shift: true, alt: true }), false)).toBeNull();
    expect(shortcutFor(press('KeyT', { shift: true }), false)).toBeNull();
  });

  it('uses Cmd instead of Ctrl on macOS', () => {
    expect(shortcutFor(press('KeyT', { meta: true, shift: true }), true)).toEqual({ kind: 'newSession' });
    expect(shortcutFor(press('KeyT', { ctrl: true, shift: true }), true)).toBeNull();
    expect(shortcutFor(press('Digit1', { meta: true }), false)).toBeNull();
  });

  it('changes the text size with Ctrl and +, = or -, and 0 for normal, on any keyboard layout', () => {
    const bigger = { kind: 'textSize', change: 'bigger' };
    const smaller = { kind: 'textSize', change: 'smaller' };
    expect(shortcutFor(press('Equal', { ctrl: true }, '='), false)).toEqual(bigger);
    expect(shortcutFor(press('Equal', { ctrl: true, shift: true }, '+'), false)).toEqual(bigger); // US
    expect(shortcutFor(press('BracketRight', { ctrl: true }, '+'), false)).toEqual(bigger); // German
    expect(shortcutFor(press('NumpadAdd', { ctrl: true }, '+'), false)).toEqual(bigger);
    expect(shortcutFor(press('Minus', { ctrl: true }, '-'), false)).toEqual(smaller);
    expect(shortcutFor(press('Slash', { ctrl: true }, '-'), false)).toEqual(smaller); // German
    expect(shortcutFor(press('Digit0', { ctrl: true }), false)).toEqual({ kind: 'textSize', change: 'reset' });
    expect(shortcutFor(press('Equal', { meta: true }, '='), true)).toEqual(bigger);
    // Ctrl+_ (Claude Code's undo), Shift+0 (= on a German keyboard) and plain keys stay with the terminal.
    expect(shortcutFor(press('Minus', { ctrl: true, shift: true }, '_'), false)).toBeNull();
    expect(shortcutFor(press('Digit0', { ctrl: true, shift: true }, '='), false)).toBeNull();
    expect(shortcutFor(press('Minus', {}, '-'), false)).toBeNull();
  });
});

describe('fontSizeAfter', () => {
  it('steps by one within 8 to 32, and goes back to 13', () => {
    expect(fontSizeAfter(13, 'bigger')).toBe(14);
    expect(fontSizeAfter(13, 'smaller')).toBe(12);
    expect(fontSizeAfter(32, 'bigger')).toBe(32);
    expect(fontSizeAfter(8, 'smaller')).toBe(8);
    expect(fontSizeAfter(20, 'reset')).toBe(13);
  });
});

describe('shortcutLabel', () => {
  it('writes shortcuts the platform way', () => {
    expect(shortcutLabel('T', { ctrl: 'Strg', shift: 'Umschalt' }, false)).toBe('Strg+Umschalt+T');
    expect(shortcutLabel('1…9', { ctrl: 'Ctrl', shift: 'Shift' }, false, false)).toBe('Ctrl+1…9');
    expect(shortcutLabel('T', { ctrl: 'Ctrl', shift: 'Shift' }, true)).toBe('⌘⇧T');
  });
});
