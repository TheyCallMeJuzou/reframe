// Group colors and the automatic name/color for new groups.
import type { MessageKey } from './i18n';

export const GROUP_COLORS = [
  { hex: '#d97757', label: 'color.orange' },
  { hex: '#5b9cf0', label: 'color.blue' },
  { hex: '#4fbf7f', label: 'color.green' },
  { hex: '#a67ff0', label: 'color.purple' },
  { hex: '#e06fa8', label: 'color.pink' },
  { hex: '#e0b04a', label: 'color.yellow' },
  { hex: '#3fbfbf', label: 'color.teal' },
  { hex: '#e06060', label: 'color.red' },
] as const satisfies readonly { hex: string; label: MessageKey }[];

export function isGroupColor(value: unknown): value is string {
  return GROUP_COLORS.some((c) => c.hex === value);
}

/** The first color no existing group uses; once all are taken, they repeat in order. */
export function nextGroupColor(used: string[]): string {
  const free = GROUP_COLORS.find((c) => !used.includes(c.hex));
  return (free ?? GROUP_COLORS[used.length % GROUP_COLORS.length]!).hex;
}

/** The lowest number n for which `nameFor(n)` is not taken yet ("Group 1", "Group 2", …). */
export function nextGroupNumber(used: string[], nameFor: (n: number) => string): number {
  let n = 1;
  while (used.includes(nameFor(n))) n++;
  return n;
}
