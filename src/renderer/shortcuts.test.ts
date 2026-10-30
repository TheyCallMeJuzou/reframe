import { describe, expect, it } from 'vitest';
import {
  changeFontSize,
  echoFontSize,
  fontSizeAfter,
  shortcutFor,
  shortcutLabel,
  type FontSizeState,
} from './shortcuts';

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
    expect(shortcutFor(press('Space', { ctrl: true, shift: true }, ' '), false)).toEqual({ kind: 'nextWaiting' });
    expect(shortcutFor(press('Space', { meta: true, shift: true }, ' '), true)).toEqual({ kind: 'nextWaiting' });
    expect(shortcutFor(press('Space', { ctrl: true }, ' '), false)).toBeNull();
    expect(shortcutFor(press('KeyF', { ctrl: true, shift: true }), false)).toEqual({ kind: 'find' });
    expect(shortcutFor(press('KeyZ', { ctrl: true, shift: true }), false)).toEqual({ kind: 'zoom' });
    expect(shortcutFor(press('ArrowLeft', { ctrl: true, shift: true }), false)).toEqual({
      kind: 'focusDirection',
      direction: 'left',
    });
    expect(shortcutFor(press('Digit3', { ctrl: true }), false)).toEqual({ kind: 'focusTile', index: 2 });
    expect(shortcutFor(press('F11'), false)).toEqual({ kind: 'toggleFullscreen' });
    expect(shortcutFor(press('Comma', { ctrl: true }, ','), false)).toEqual({ kind: 'openSettings' });
    expect(shortcutFor(press('Comma', { meta: true }, ','), true)).toEqual({ kind: 'openSettings' });
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

describe('changeFontSize and echoFontSize', () => {
  it('changes at once and takes a saved size back only once no change of its own is on its way', () => {
    // Ctrl+= twice, then Ctrl+-: the echo of the first press (14) arrives in between and must not set 15 back.
    let state: FontSizeState = { size: 13, awaited: null };
    state = changeFontSize(state, 'bigger')!;
    state = changeFontSize(state, 'bigger')!;
    expect(state).toEqual({ size: 15, awaited: 15 });
    expect(echoFontSize(state, 14)).toBe(state);
    state = changeFontSize(state, 'smaller')!;
    expect(state.size).toBe(14);
    expect(echoFontSize(echoFontSize(state, 15), 14)).toEqual({ size: 14, awaited: null });
  });

  it('takes another window’s change when nothing is on its way, and has no change at the end of the range', () => {
    expect(echoFontSize({ size: 14, awaited: null }, 20)).toEqual({ size: 20, awaited: null });
    expect(changeFontSize({ size: 32, awaited: null }, 'bigger')).toBeNull();
    expect(changeFontSize({ size: 13, awaited: null }, 'reset')).toBeNull();
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
