// reframe's keyboard shortcuts. They use Ctrl+Shift (Cmd+Shift on macOS), plus Ctrl+1…9, Ctrl+, (settings), F11 and the text size keys
// (Ctrl with +, - and 0, as in Windows Terminal), because Claude Code itself takes many plain Ctrl keys; reframe
// catches these before the terminal sees them.
import { FONT_SIZE } from '@shared/state';
import type { Direction } from './focus-nav';

export type TextSizeChange = 'bigger' | 'smaller' | 'reset';

export type Shortcut =
  | { kind: 'focusTile'; index: number }
  | { kind: 'focusDirection'; direction: Direction }
  | { kind: 'newSession' }
  | { kind: 'newWindow' }
  | { kind: 'nextWaiting' }
  | { kind: 'find' }
  | { kind: 'zoom' }
  | { kind: 'toggleCollapse' }
  | { kind: 'toggleSidebar' }
  | { kind: 'toggleFullscreen' }
  | { kind: 'openSettings' }
  | { kind: 'textSize'; change: TextSizeChange };

interface Keys {
  code: string;
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

const ARROWS: Record<string, Direction> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
const LETTERS: Record<string, Shortcut> = {
  KeyT: { kind: 'newSession' },
  KeyN: { kind: 'newWindow' },
  KeyM: { kind: 'toggleCollapse' },
  KeyL: { kind: 'toggleSidebar' },
  Space: { kind: 'nextWaiting' },
  KeyF: { kind: 'find' },
  KeyZ: { kind: 'zoom' },
};
// By character, not key: they sit elsewhere on other layouts (+ has a key of its own on a German keyboard).
const TEXT_SIZE: Record<string, TextSizeChange> = { '+': 'bigger', '=': 'bigger', '-': 'smaller', '0': 'reset' };

/** The shortcut a key press means, if any (by physical key, so it does not depend on the keyboard layout). */
export function shortcutFor(keys: Keys, mac: boolean): Shortcut | null {
  if (keys.code === 'F11' && !keys.ctrlKey && !keys.metaKey && !keys.shiftKey && !keys.altKey) {
    return { kind: 'toggleFullscreen' };
  }
  const mod = mac ? keys.metaKey && !keys.ctrlKey : keys.ctrlKey && !keys.metaKey;
  if (!mod || keys.altKey) return null;
  // Shift only for + (Shift and = on US keyboards); Ctrl+Shift+- is Ctrl+_, Claude Code's undo.
  const change = TEXT_SIZE[keys.key];
  if (change && (!keys.shiftKey || keys.key === '+')) return { kind: 'textSize', change };
  if (keys.code === 'Comma' && !keys.shiftKey) return { kind: 'openSettings' };
  const digit = /^Digit([1-9])$/.exec(keys.code);
  if (digit && !keys.shiftKey) return { kind: 'focusTile', index: Number(digit[1]) - 1 };
  if (!keys.shiftKey) return null;
  const direction = ARROWS[keys.code];
  if (direction) return { kind: 'focusDirection', direction };
  return LETTERS[keys.code] ?? null;
}

/** The font size a text size change leads to, within the range. */
export function fontSizeAfter(size: number, change: TextSizeChange): number {
  if (change === 'reset') return FONT_SIZE.normal;
  return Math.min(FONT_SIZE.max, Math.max(FONT_SIZE.min, size + (change === 'bigger' ? 1 : -1)));
}

/** How a shortcut is written in hints, e.g. "Ctrl+Shift+T" ("Strg+Umschalt+T"), or "⌘⇧T" on macOS. */
export function shortcutLabel(key: string, words: { ctrl: string; shift: string }, mac: boolean, shift = true): string {
  if (mac) return `⌘${shift ? '⇧' : ''}${key}`;
  return [words.ctrl, ...(shift ? [words.shift] : []), key].join('+');
}
