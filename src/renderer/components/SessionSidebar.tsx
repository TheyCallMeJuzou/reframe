import { useEffect, useMemo, useState } from 'react';
import type { AppListItem, FoundListItem, ListItemRef, RunningListItem, SessionList } from '@shared/ipc';
import { tileKey } from '@shared/layout';
import { useApp } from '../context';
import { setListDrag, setNodeDrag } from '../dnd';
import { CloseIcon } from './icons';
import { StatusDot } from './StatusDot';

const PAGE = 20;
const REFRESH_MS = 3000;

const folderName = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

/** All sessions: the ones reframe manages, and ones found outside it (running or past). */
export function SessionSidebar() {
  const { t, locale } = useApp();
  const [query, setQuery] = useState('');
  const [limit, setLimit] = useState(PAGE);
  const [list, setList] = useState<SessionList | null>(null);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    let alive = true;
    const load = () =>
      void window.reframe.getSessionList(query, limit).then((next) => {
        if (alive) setList(next);
      });
    const soon = setTimeout(load, 150); // typing in the search box
    const every = setInterval(load, REFRESH_MS);
    return () => {
      alive = false;
      clearTimeout(soon);
      clearInterval(every);
    };
  }, [query, limit, reload]);

  const ago = useMemo(() => {
    const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
    return (time: number) => {
      const minutes = Math.round((time - Date.now()) / 60_000);
      if (Math.abs(minutes) < 60) return format.format(minutes, 'minute');
      const hours = Math.round(minutes / 60);
      if (Math.abs(hours) < 24) return format.format(hours, 'hour');
      return format.format(Math.round(hours / 24), 'day');
    };
  }, [locale]);

  const open = (item: ListItemRef) => {
    window.reframe.openSession({ item, target: null });
    setTimeout(() => setReload((n) => n + 1), 300);
  };

  return (
    <aside className="sidebar" aria-label={t('list.label')}>
      <input
        className="sidebar-search"
        type="search"
        value={query}
        placeholder={t('list.search')}
        aria-label={t('list.search')}
        onChange={(event) => {
          setQuery(event.target.value);
          setLimit(PAGE);
        }}
      />
      <div className="sidebar-scroll">
        <h3 className="sidebar-heading">{t('list.inApp')}</h3>
        {list?.app.length === 0 && <p className="sidebar-empty">{t('list.emptyApp')}</p>}
        {list?.app.map((item) => (
          <AppRow
            key={item.id}
            item={item}
            onOpen={() => open({ kind: 'app', sessionId: item.id })}
            onRemove={() => {
              window.reframe.removeSession(item.id);
              setTimeout(() => setReload((n) => n + 1), 100);
            }}
          />
        ))}

        <h3 className="sidebar-heading">{t('list.found')}</h3>
        {list && list.running.length + list.found.length === 0 && (
          <p className="sidebar-empty">{t('list.emptyFound')}</p>
        )}
        {list?.running.map((item) => (
          <RunningRow
            key={item.pid}
            item={item}
            onOpen={() => open({ kind: 'running', pid: item.pid, claudeSessionId: item.claudeSessionId })}
          />
        ))}
        {list?.found.map((item) => (
          <FoundRow
            key={item.claudeSessionId}
            item={item}
            when={ago(item.modifiedAt)}
            onOpen={() => open({ kind: 'found', claudeSessionId: item.claudeSessionId })}
          />
        ))}
        {list?.hasMore && (
          <button className="button sidebar-more" onClick={() => setLimit((n) => n + PAGE)}>
            {t('list.more')}
          </button>
        )}
      </div>
    </aside>
  );
}

function AppRow({ item, onOpen, onRemove }: { item: AppListItem; onOpen: () => void; onRemove: () => void }) {
  const { t } = useApp();
  const meta = item.windowNumber ? t('list.window', { n: item.windowNumber }) : t('list.ended');
  return (
    <div className="sidebar-row-wrap">
      <button
        className="sidebar-row"
        data-session={item.id}
        title={item.windowId ? t('list.showHint') : t('list.openHint')}
        draggable
        onDragStart={(event) => {
          // A session shown in a window is moved like its tile; one in no window is opened where it lands.
          if (item.windowId) setNodeDrag(event, 'tile', { key: tileKey(item.id), windowId: item.windowId });
          else setListDrag(event, { kind: 'app', sessionId: item.id });
        }}
        onClick={onOpen}
      >
        <StatusDot status={item.status} />
        <span className="sidebar-text">
          <span className="sidebar-title">{item.label}</span>
          <span className="sidebar-meta">
            {folderName(item.cwd)} · {meta}
          </span>
        </span>
      </button>
      {!item.windowId && (
        <button
          className="icon-button sidebar-remove"
          title={t('list.remove')}
          aria-label={t('list.remove')}
          onClick={onRemove}
        >
          <CloseIcon />
        </button>
      )}
    </div>
  );
}

function RunningRow({ item, onOpen }: { item: RunningListItem; onOpen: () => void }) {
  const { t } = useApp();
  return (
    <button
      className="sidebar-row"
      title={t('list.openHint')}
      draggable
      onDragStart={(event) =>
        setListDrag(event, { kind: 'running', pid: item.pid, claudeSessionId: item.claudeSessionId })
      }
      onClick={onOpen}
    >
      <span className="running-mark" aria-hidden="true">
        ▶
      </span>
      <span className="sidebar-text">
        <span className="sidebar-title">{item.name ?? folderName(item.cwd)}</span>
        <span className="sidebar-meta">
          {folderName(item.cwd)} · {t('list.running')}
        </span>
      </span>
    </button>
  );
}

function FoundRow({ item, when, onOpen }: { item: FoundListItem; when: string; onOpen: () => void }) {
  const { t } = useApp();
  return (
    <button
      className="sidebar-row"
      title={`${item.cwd}\n${t('list.openHint')}`}
      draggable
      onDragStart={(event) => setListDrag(event, { kind: 'found', claudeSessionId: item.claudeSessionId })}
      onClick={onOpen}
    >
      <StatusDot status="ended" />
      <span className="sidebar-text">
        <span className="sidebar-title">{item.title ?? folderName(item.cwd)}</span>
        <span className="sidebar-meta">
          {folderName(item.cwd)} · {when}
        </span>
      </span>
    </button>
  );
}
