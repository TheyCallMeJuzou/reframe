// The session list, kept up to date while the window is open (more often while the list is shown), and which
// sessions popped up since: started in another terminal, or a past one that was just used again.
import { useEffect, useRef, useState } from 'react';
import type { SessionList } from '@shared/ipc';

export const PAGE = 20;
const REFRESH_MS = 3000;
const HIDDEN_REFRESH_MS = 6000;
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

  useEffect(() => {
    let alive = true;
    if (since.current === 0) since.current = Date.now();
    const load = () =>
      void window.reframe.getSessionList(shown ? query : '', shown ? limit : PAGE).then((next) => {
        if (!alive) return;
        setList(next);
        // New: not seen before, and running now or used since reframe started (paging and searching only
        // reveal older ones). The first load is what was there already.
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
      });
    const soon = setTimeout(load, 150); // typing in the search box
    const every = setInterval(load, shown ? REFRESH_MS : HIDDEN_REFRESH_MS);
    return () => {
      alive = false;
      clearTimeout(soon);
      clearInterval(every);
    };
  }, [shown, query, limit, reloads]);

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
