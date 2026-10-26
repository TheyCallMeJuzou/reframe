// Sessions that need the user (Claude asks for permission, or waits for an answer): which one is next.

/**
 * The next session in `order` after `after` that is waiting, going round (`after` itself only when no other one
 * is); from the start of `order` without `after`. Null when none is waiting.
 */
export function nextWaiting(order: string[], waiting: ReadonlySet<string>, after: string | null): string | null {
  const start = after ? order.indexOf(after) : -1;
  for (let step = 1; step <= order.length; step++) {
    const id = order[(start + step) % order.length]!;
    if (waiting.has(id)) return id;
  }
  return null;
}
