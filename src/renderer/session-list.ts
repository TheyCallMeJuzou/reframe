// The session list, kept up to date while the window is open, and which sessions popped up since: started in another
// terminal, or a past one that was just used again. Main sends the first page whenever it changed; a search or more
// pages are asked for (and again whenever the list changed).
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { LIST_PAGE, type SessionList } from '@shared/ipc';

export const PAGE = LIST_PAGE;
/** How long a new entry stays highlighted once the list is shown. */
const HIGHLIGHT_MS = 4000;

export interface SessionListState {
  list: SessionList | null;
  query: string;
  setQuery: (query: string) => void;
  limit: number;
  setLimit: (update: (limit: number) => number) => void;
  /** Claude session ids of entries that are new since the list was last looked at. */
  fresh: ReadonlySet<string>;
  reload: () => void;
}

export function useSessionList(shown: boolean): SessionListState {
  const [query, setQuery] = useState('');
  const [limit, setLimit] = useState(PAGE);
  const [list, setList] = useState<SessionList | null>(null);
  const [reloads, setReloads] = useState(0);
  const [fresh, setFresh] = useState<ReadonlySet<string>>(new Set());
  const seen = useRef<Set<string> | null>(null);
  /** When this window started watching; 0 until the first load. */
  const since = useRef(0);
  /** Counts lists main sent: an answer asked for before the latest of them is older than it. */
  const sent = useRef(0);

  // A search and more pages only while the list is shown; hidden, the first page is enough (for the dot).
  const asked = shown ? { query, limit } : { query: '', limit: PAGE };
  const askedRef = useRef(asked);
  useLayoutEffect(() => {
    askedRef.current = asked;
  });

  const apply = useCallback((next: SessionList) => {
    setList(next);
    // New: not seen before, and running now or used since reframe started (paging and searching only reveal older
    // ones). The first load is what was there already.
    const entries = [
      ...next.running.map((r) => ({ id: r.claudeSessionId, recent: true })),
      ...next.found.map((f) => ({ id: f.claudeSessionId, recent: f.modifiedAt > since.current })),
    ];
    const known = seen.current;
    if (!known) {
      seen.current = new Set(entries.map((e) => e.id));
      return;
    }
    const added = entries.filter((e) => !known.has(e.id));
    for (const e of added) known.add(e.id);
    const popped = added.filter((e) => e.recent).map((e) => e.id);
    if (popped.length > 0) setFresh((current) => new Set([...current, ...popped]));
  }, []);

  useEffect(
    () =>
      window.reframe.onSessionList((next) => {
        sent.current++;
        const { query: q, limit: l } = askedRef.current;
        if (!q && l === PAGE) apply(next);
        else setReloads((n) => n + 1);
      }),
    [apply],
  );

  // Also each time the list is shown again (`shown`): main looks at what runs outside reframe afresh for a list
  // asked for, and the page it sends is otherwise up to a little while old. Hidden, that page is enough.
  useEffect(() => {
    let alive = true;
    if (since.current === 0) since.current = Date.now();
    const before = sent.current;
    // A moment's wait while typing in the search box.
    const timer = setTimeout(
      () =>
        void window.reframe.getSessionList(asked.query, asked.limit).then((next) => {
          if (alive && sent.current === before) apply(next);
        }),
      150,
    );
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [asked.query, asked.limit, reloads, apply, shown]);

  // Once the list is shown, new entries light up for a moment and then count as seen.
  useEffect(() => {
    if (!shown || fresh.size === 0) return;
    const timer = setTimeout(() => setFresh(new Set()), HIGHLIGHT_MS);
    return () => clearTimeout(timer);
  }, [shown, fresh]);

  return {
    list,
    query,
    setQuery,
    limit,
    setLimit,
    fresh,
    reload: () => setReloads((n) => n + 1),
  };
}
