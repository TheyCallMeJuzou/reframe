import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ForeignDropRequest, SessionRename, WindowView } from '@shared/ipc';
import type { DropTarget } from '@shared/layout';
import { AppContext, makeTranslate, type AppContextValue, type DragState } from './context';
import { dragKind, dropTargetAt, handleDrop, type PreviewRect } from './dnd';
import type { TerminalRegistry } from './terminals';
import { DockStrip } from './components/DockStrip';
import { NewSessionDialog } from './components/NewSessionDialog';
import { LayoutView } from './components/SplitView';
import { InfoIcon, ListIcon, PlusIcon, WindowIcon } from './components/icons';
import { AboutDialog } from './components/AboutDialog';
import { neighbor, type Box } from './focus-nav';
import { useSessionList } from './session-list';
import { shortcutFor, shortcutLabel, type Shortcut } from './shortcuts';
import { SessionSidebar } from './components/SessionSidebar';
import { TakeoverDialog } from './components/TakeoverDialog';

const MAC = navigator.userAgent.includes('Mac');

const tileIds = () =>
  [...document.querySelectorAll<HTMLElement>('.layout .tile[data-session]')].map((el) => el.dataset.session!);
const activeTile = () => document.activeElement?.closest<HTMLElement>('.tile[data-session]')?.dataset.session ?? null;

/** Runs `action` once `selector` is on the page (it may only appear with the next view), giving up after a while. */
function whenShown(selector: string, action: () => void, frames = 60): void {
  if (document.querySelector(selector)) return action();
  if (frames > 0) requestAnimationFrame(() => whenShown(selector, action, frames - 1));
}

export function App({ terminals }: { terminals: TerminalRegistry }) {
  const [view, setView] = useState<WindowView | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [dragging, setDragging] = useState<DragState | null>(null);
  const [emptyDropActive, setEmptyDropActive] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renamingSession, setRenamingSession] = useState<SessionRename | null>(null);
  // A terminal window from another app dragged over this window, and dropped on it.
  const [foreignPreview, setForeignPreview] = useState<PreviewRect | null>(null);
  const [foreignDrop, setForeignDrop] = useState<{ request: ForeignDropRequest; target: DropTarget | null } | null>(
    null,
  );

  useEffect(() => {
    let pushed = false;
    const off = window.reframe.onView((next) => {
      pushed = true;
      setView(next);
    });
    void window.reframe.getView().then((initial) => {
      if (initial && !pushed) setView(initial);
    });
    return off;
  }, []);

  useEffect(() => {
    if (view) terminals.retain(new Set(Object.keys(view.sessions)));
  }, [view, terminals]);

  const sessionList = useSessionList(!!view?.sidebarOpen);
  const newFound = !view?.sidebarOpen && sessionList.fresh.size > 0;

  // Keyboard shortcuts, caught before the terminal (or anything else) sees the key.
  // Updated right after rendering (before the next key press is handled), so a quick second press sees it.
  const viewRef = useRef(view);
  useLayoutEffect(() => {
    viewRef.current = view;
  }, [view]);
  const focusTile = useCallback(
    (sessionId: string) =>
      whenShown(`.tile[data-session="${CSS.escape(sessionId)}"]`, () => terminals.focus(sessionId)),
    [terminals],
  );
  const runShortcut = useCallback(
    (shortcut: Shortcut) => {
      const current = viewRef.current;
      if (!current) return;
      const ids = tileIds();
      switch (shortcut.kind) {
        case 'focusTile': {
          const id = ids[shortcut.index];
          if (id) terminals.focus(id);
          return;
        }
        case 'focusDirection': {
          const from = activeTile();
          if (!from) return void (ids[0] && terminals.focus(ids[0]));
          const boxes: Box[] = ids.map((id) => {
            const rect = document.querySelector(`.tile[data-session="${CSS.escape(id)}"]`)!.getBoundingClientRect();
            return { id, x: rect.x, y: rect.y, width: rect.width, height: rect.height };
          });
          const next = neighbor(boxes, from, shortcut.direction);
          if (next) terminals.focus(next);
          return;
        }
        case 'newSession':
          return setDialogOpen(true);
        case 'newWindow':
          return window.reframe.newWindow();
        case 'toggleFullscreen':
          return window.reframe.toggleFullscreen();
        case 'toggleCollapse': {
          // In a tile: collapse it. Otherwise: bring back the one collapsed last.
          const active = activeTile();
          if (active) return window.reframe.dispatch({ type: 'tile/collapse', sessionId: active });
          const last = current.dock[current.dock.length - 1];
          if (last?.kind === 'tile') {
            window.reframe.dispatch({ type: 'tile/expand', sessionId: last.sessionId });
            focusTile(last.sessionId);
          } else if (last?.kind === 'group') {
            window.reframe.dispatch({ type: 'group/expand', groupId: last.node.id });
          }
          return;
        }
        case 'toggleSidebar': {
          const open = !current.sidebarOpen;
          window.reframe.dispatch({ type: 'window/sidebar', open });
          if (open)
            whenShown('.sidebar-search', () => document.querySelector<HTMLInputElement>('.sidebar-search')?.focus());
          return;
        }
      }
    },
    [terminals, focusTile],
  );
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (document.querySelector('.modal')) return; // dialogs keep their keys
      const shortcut = shortcutFor(event, MAC);
      if (!shortcut) return;
      event.preventDefault();
      event.stopPropagation();
      runShortcut(shortcut);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [runShortcut]);

  useEffect(
    () =>
      window.reframe.onFocusTile((sessionId) => {
        // The tile may only appear with the next view (when it was collapsed), so wait a frame.
        requestAnimationFrame(() => {
          terminals.focus(sessionId);
          const el = document.querySelector(`.tile[data-session="${CSS.escape(sessionId)}"]`);
          el?.classList.add('flash');
          setTimeout(() => el?.classList.remove('flash'), 900);
        });
      }),
    [terminals],
  );

  useEffect(() => window.reframe.onOpenNewSession(() => setDialogOpen(true)), []);
  useEffect(() => window.reframe.onOpenAbout(() => setAboutOpen(true)), []);
  useEffect(() => window.reframe.onRenameGroup(setRenaming), []);
  useEffect(() => window.reframe.onRenameSession(setRenamingSession), []);
  useEffect(
    () => window.reframe.onForeignDrag((point) => setForeignPreview(point && dropTargetAt(point.x, point.y).rect)),
    [],
  );
  useEffect(
    () =>
      window.reframe.onForeignDrop((request) => {
        // Taken before the dialog covers the window.
        setForeignPreview(null);
        setForeignDrop({ request, target: dropTargetAt(request.x, request.y).target });
      }),
    [],
  );

  // Drag state: tiles and groups from other windows announce themselves only through dragenter. A drop
  // re-parents or removes the dragged node, so its own dragend may never reach React; several events reset it.
  useEffect(() => {
    const reset = () => setDragging(null);
    const enter = (event: DragEvent) => {
      const kind = dragKind(event);
      if (kind) setDragging((current) => current ?? { key: null, kind });
    };
    const leave = (event: DragEvent) => {
      // relatedTarget is null when the pointer leaves the window.
      if (event.relatedTarget === null) setDragging((current) => (current?.key === null ? null : current));
    };
    // Pointer events are suppressed during a drag, so any pointer movement means it is over.
    const pointer = () => setDragging((current) => (current ? null : current));
    // Drags from other apps (a terminal tab torn out over us opens a new terminal window there): main shows
    // drop zones for them and uses the last position as the drop point.
    let reported = 0;
    const over = (event: DragEvent) => {
      if (dragKind(event) || event.timeStamp - reported < 50) return;
      reported = event.timeStamp;
      const types = Array.from(event.dataTransfer?.types ?? []);
      window.reframe.reportForeignPointer({ x: event.screenX, y: event.screenY, types });
    };
    const out = (event: DragEvent) => {
      if (!dragKind(event) && (event.type === 'drop' || event.relatedTarget === null)) {
        window.reframe.reportForeignPointer(null);
      }
    };
    window.addEventListener('dragover', over, true);
    window.addEventListener('dragleave', out, true);
    window.addEventListener('drop', out, true);
    window.addEventListener('dragenter', enter, true);
    window.addEventListener('dragleave', leave, true);
    // Bubble phase on purpose: resetting during capture re-renders (in the microtask after this listener)
    // before React's own drop handler runs, which then finds the drop layer gone.
    window.addEventListener('drop', reset);
    window.addEventListener('dragend', reset, true);
    window.addEventListener('pointermove', pointer, true);
    return () => {
      window.removeEventListener('dragenter', enter, true);
      window.removeEventListener('dragleave', leave, true);
      window.removeEventListener('drop', reset);
      window.removeEventListener('dragend', reset, true);
      window.removeEventListener('pointermove', pointer, true);
      window.removeEventListener('dragover', over, true);
      window.removeEventListener('dragleave', out, true);
      window.removeEventListener('drop', out, true);
    };
  }, []);

  const context = useMemo<AppContextValue | null>(
    () =>
      view && {
        t: makeTranslate(view.locale),
        keys: (key: string, shift = true) => {
          const t = makeTranslate(view.locale);
          return shortcutLabel(key, { ctrl: t('keys.ctrl'), shift: t('keys.shift') }, MAC, shift);
        },
        locale: view.locale,
        windowId: view.windowId,
        sessions: view.sessions,
        terminals,
        dragging,
        setDragging,
        renaming,
        setRenaming,
        renamingSession,
        setRenamingSession,
      },
    [view, terminals, dragging, renaming, renamingSession],
  );

  if (!view || !context) return null;
  const { t, keys } = context;

  return (
    <AppContext.Provider value={context}>
      <div className="app">
        <div className="toolbar">
          <button
            className="button toolbar-button"
            title={`${t('toolbar.newSession')} (${keys('T')})`}
            onClick={() => setDialogOpen(true)}
          >
            <PlusIcon />
            {t('toolbar.newSession')}
          </button>
          <button
            className={`button toolbar-button${view.sidebarOpen ? ' is-active' : ''}`}
            aria-pressed={view.sidebarOpen}
            title={`${t('toolbar.sessions')} (${keys('L')})`}
            onClick={() => window.reframe.dispatch({ type: 'window/sidebar', open: !view.sidebarOpen })}
          >
            <ListIcon />
            {t('toolbar.sessions')}
            {newFound && <span className="toolbar-dot" title={t('list.newFound')} aria-hidden="true" />}
          </button>
          <button
            className="button toolbar-button"
            title={`${t('toolbar.newWindow')} (${keys('N')})`}
            onClick={() => window.reframe.newWindow()}
          >
            <WindowIcon />
            {t('toolbar.newWindow')}
          </button>
          <span className="spacer" />
          <button
            className="icon-button toolbar-icon"
            title={t('about.title')}
            aria-label={t('about.title')}
            onClick={() => setAboutOpen(true)}
          >
            <InfoIcon />
          </button>
        </div>
        <div className="workspace">
          {view.sidebarOpen && <SessionSidebar sessionList={sessionList} />}
          <DockStrip entries={view.dock} />
          <main className="layout">
            {view.layout ? (
              <LayoutView node={view.layout} />
            ) : (
              <div className="empty-area">
                <div className="empty">
                  <h1>{t('empty.title')}</h1>
                  <p>{t('empty.hint')}</p>
                  <button className="button primary" onClick={() => setDialogOpen(true)}>
                    <PlusIcon />
                    {t('toolbar.newSession')}
                  </button>
                </div>
                {dragging && (
                  <div
                    className="drop-layer"
                    onDragOver={(event) => {
                      if (!dragKind(event)) return;
                      event.preventDefault();
                      event.dataTransfer.dropEffect = 'move';
                      setEmptyDropActive(true);
                    }}
                    onDragLeave={() => setEmptyDropActive(false)}
                    onDrop={(event) => {
                      setEmptyDropActive(false);
                      if (handleDrop(event, view.windowId, null)) event.preventDefault();
                    }}
                  >
                    {emptyDropActive && <div className="drop-preview drop-fill" />}
                  </div>
                )}
              </div>
            )}
          </main>
        </div>
        {dialogOpen && <NewSessionDialog onClose={() => setDialogOpen(false)} />}
        {aboutOpen && (
          <AboutDialog
            version={view.version}
            language={view.language}
            systemLocale={view.systemLocale}
            onClose={() => setAboutOpen(false)}
          />
        )}
        {foreignPreview && <div className="drop-preview foreign-preview" style={foreignPreview} />}
        {foreignDrop && (
          <TakeoverDialog
            request={foreignDrop.request}
            target={foreignDrop.target}
            onDone={() => setForeignDrop(null)}
          />
        )}
      </div>
    </AppContext.Provider>
  );
}
