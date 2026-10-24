import { useMemo, useRef, useState, type DragEvent, type MouseEvent, type ReactNode } from 'react';
import type { ListItemRef } from '@shared/ipc';
import { tileKey } from '@shared/layout';
import { useApp } from '../context';
import { setListDrag, setNodeDrag } from '../dnd';
import { PAGE, type SessionListState } from '../session-list';
import { CloseIcon } from './icons';
import { StatusDot } from './StatusDot';

const folderName = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;

interface Row {
  key: string;
  ref: ListItemRef;
}

/**
 * All sessions: the ones reframe manages, and ones found outside it (running or past). A click opens one;
 * Shift+click selects a range and Ctrl+click (Cmd on macOS) single entries, to open them together from the
 * right-click menu or by dragging them into a window.
 */
export function SessionSidebar({ sessionList }: { sessionList: SessionListState }) {
  const { t, locale } = useApp();
  const { list, query, setQuery, setLimit, fresh, reload } = sessionList;
  const [selected, setSelected] = useState<string[]>([]);
  const anchor = useRef<string | null>(null);

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

  const rows = useMemo<Row[]>(
    () =>
      list
        ? [
            ...list.app.map((i) => ({ key: `app:${i.id}`, ref: { kind: 'app' as const, sessionId: i.id } })),
            ...list.running.map((i) => ({
              key: `claude:${i.claudeSessionId}`,
              ref: { kind: 'running' as const, pid: i.pid, claudeSessionId: i.claudeSessionId },
            })),
            ...list.found.map((i) => ({
              key: `claude:${i.claudeSessionId}`,
              ref: { kind: 'found' as const, claudeSessionId: i.claudeSessionId },
            })),
          ]
        : [],
    [list],
  );
  const chosen = selected.filter((key) => rows.some((r) => r.key === key));
  const refsOf = (keys: string[]) => rows.filter((r) => keys.includes(r.key)).map((r) => r.ref);

  const open = (item: ListItemRef) => {
    window.reframe.openSession({ item, target: null });
    setTimeout(reload, 300);
  };

  const onRowClick = (row: Row, event: MouseEvent) => {
    const from = anchor.current ? rows.findIndex((r) => r.key === anchor.current) : -1;
    if (event.shiftKey && from >= 0) {
      const to = rows.findIndex((r) => r.key === row.key);
      setSelected(rows.slice(Math.min(from, to), Math.max(from, to) + 1).map((r) => r.key));
      return;
    }
    anchor.current = row.key;
    if (event.ctrlKey || event.metaKey) {
      setSelected((keys) => (keys.includes(row.key) ? keys.filter((k) => k !== row.key) : [...keys, row.key]));
      return;
    }
    setSelected([row.key]);
    open(row.ref);
  };
  const onRowMenu = (row: Row, event: MouseEvent) => {
    event.preventDefault();
    const keys = chosen.includes(row.key) ? chosen : [row.key];
    if (!chosen.includes(row.key)) {
      setSelected(keys);
      anchor.current = row.key;
    }
    window.reframe.showListMenu(refsOf(keys));
    setTimeout(reload, 600);
  };
  /** Dragging one of several selected entries takes all of them along. */
  const onRowDrag = (row: Row, event: DragEvent, single: () => void) => {
    if (chosen.length > 1 && chosen.includes(row.key)) setListDrag(event, refsOf(chosen));
    else single();
  };
  const rowProps = (row: Row) => ({
    selected: chosen.includes(row.key),
    onClick: (event: MouseEvent) => onRowClick(row, event),
    onContextMenu: (event: MouseEvent) => onRowMenu(row, event),
  });

  return (
    <aside
      className="sidebar"
      aria-label={t('list.label')}
      onKeyDown={(event) => event.key === 'Escape' && setSelected([])}
    >
      <input
        className="sidebar-search"
        type="search"
        value={query}
        placeholder={t('list.search')}
        aria-label={t('list.search')}
        onChange={(event) => {
          setQuery(event.target.value);
          setLimit(() => PAGE);
        }}
      />
      <div className="sidebar-scroll">
        <h3 className="sidebar-heading">{t('list.inApp')}</h3>
        {list?.app.length === 0 && <p className="sidebar-empty">{t('list.emptyApp')}</p>}
        {list?.app.map((item) => {
          const row: Row = { key: `app:${item.id}`, ref: { kind: 'app', sessionId: item.id } };
          return (
            <div className="sidebar-row-wrap" key={item.id}>
              <ListRow
                {...rowProps(row)}
                title={item.windowId ? t('list.showHint') : t('list.openHint')}
                session={item.id}
                onDragStart={(event) =>
                  onRowDrag(row, event, () => {
                    // A session shown in a window is moved like its tile; one in no window is opened where it lands.
                    if (item.windowId) setNodeDrag(event, 'tile', { key: tileKey(item.id), windowId: item.windowId });
                    else setListDrag(event, [row.ref]);
                  })
                }
                mark={<StatusDot status={item.status} />}
                label={item.label}
                meta={`${folderName(item.cwd)} · ${item.windowNumber ? t('list.window', { n: item.windowNumber }) : t('list.ended')}`}
              />
              {!item.windowId && (
                <button
                  className="icon-button sidebar-remove"
                  title={t('list.remove')}
                  aria-label={t('list.remove')}
                  onClick={() => {
                    window.reframe.removeSession(item.id);
                    setTimeout(reload, 100);
                  }}
                >
                  <CloseIcon />
                </button>
              )}
            </div>
          );
        })}

        <h3 className="sidebar-heading">{t('list.found')}</h3>
        {list && list.running.length + list.found.length === 0 && (
          <p className="sidebar-empty">{t('list.emptyFound')}</p>
        )}
        {list?.running.map((item) => {
          const row: Row = {
            key: `claude:${item.claudeSessionId}`,
            ref: { kind: 'running', pid: item.pid, claudeSessionId: item.claudeSessionId },
          };
          return (
            <ListRow
              key={item.pid}
              {...rowProps(row)}
              isNew={fresh.has(item.claudeSessionId)}
              title={t('list.openHint')}
              onDragStart={(event) => onRowDrag(row, event, () => setListDrag(event, [row.ref]))}
              mark={
                <span className="running-mark" aria-hidden="true">
                  ▶
                </span>
              }
              label={item.name ?? folderName(item.cwd)}
              meta={`${folderName(item.cwd)} · ${t('list.running')}`}
            />
          );
        })}
        {list?.found.map((item) => {
          const row: Row = {
            key: `claude:${item.claudeSessionId}`,
            ref: { kind: 'found', claudeSessionId: item.claudeSessionId },
          };
          return (
            <ListRow
              key={item.claudeSessionId}
              {...rowProps(row)}
              isNew={fresh.has(item.claudeSessionId)}
              title={`${item.cwd}\n${t('list.openHint')}`}
              onDragStart={(event) => onRowDrag(row, event, () => setListDrag(event, [row.ref]))}
              mark={<StatusDot status="ended" />}
              label={item.title ?? folderName(item.cwd)}
              meta={`${folderName(item.cwd)} · ${ago(item.modifiedAt)}`}
            />
          );
        })}
        {list?.hasMore && (
          <button className="button sidebar-more" onClick={() => setLimit((n) => n + PAGE)}>
            {t('list.more')}
          </button>
        )}
      </div>
    </aside>
  );
}

interface ListRowProps {
  selected: boolean;
  isNew?: boolean;
  session?: string;
  title: string;
  mark: ReactNode;
  label: string;
  meta: string;
  onClick: (event: MouseEvent) => void;
  onContextMenu: (event: MouseEvent) => void;
  onDragStart: (event: DragEvent) => void;
}

function ListRow(p: ListRowProps) {
  return (
    <button
      className={`sidebar-row${p.selected ? ' is-selected' : ''}${p.isNew ? ' is-new' : ''}`}
      data-session={p.session}
      aria-pressed={p.selected}
      title={p.title}
      draggable
      // Shift+click selects entries, not text.
      onMouseDown={(event) => event.shiftKey && event.preventDefault()}
      onClick={p.onClick}
      onContextMenu={p.onContextMenu}
      onDragStart={p.onDragStart}
    >
      {p.mark}
      <span className="sidebar-text">
        <span className="sidebar-title">{p.label}</span>
        <span className="sidebar-meta">{p.meta}</span>
      </span>
    </button>
  );
}
