// reframe's keyboard shortcuts. They use Ctrl+Shift (Cmd+Shift on macOS), plus Ctrl+1…9, because Claude Code
// itself takes many plain Ctrl keys; reframe catches these before the terminal sees them.
import type { Direction } from './focus-nav';

export type Shortcut =
  | { kind: 'focusTile'; index: number }
  | { kind: 'focusDirection'; direction: Direction }
  | { kind: 'newSession' }
  | { kind: 'newWindow' }
  | { kind: 'toggleCollapse' }
  | { kind: 'toggleSidebar' };

interface Keys {
  code: string;
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
};

/** The shortcut a key press means, if any (by physical key, so it does not depend on the keyboard layout). */
export function shortcutFor(keys: Keys, mac: boolean): Shortcut | null {
  const mod = mac ? keys.metaKey && !keys.ctrlKey : keys.ctrlKey && !keys.metaKey;
  if (!mod || keys.altKey) return null;
  const digit = /^Digit([1-9])$/.exec(keys.code);
  if (digit && !keys.shiftKey) return { kind: 'focusTile', index: Number(digit[1]) - 1 };
  if (!keys.shiftKey) return null;
  const direction = ARROWS[keys.code];
  if (direction) return { kind: 'focusDirection', direction };
  return LETTERS[keys.code] ?? null;
}

/** How a shortcut is written in hints, e.g. "Ctrl+Shift+T" ("Strg+Umschalt+T"), or "⌘⇧T" on macOS. */
export function shortcutLabel(key: string, words: { ctrl: string; shift: string }, mac: boolean, shift = true): string {
  if (mac) return `⌘${shift ? '⇧' : ''}${key}`;
  return [words.ctrl, ...(shift ? [words.shift] : []), key].join('+');
}
