import { useEffect, useMemo, useState } from 'react';
import type { ForeignDropRequest, WindowView } from '@shared/ipc';
import type { DropTarget } from '@shared/layout';
import { AppContext, makeTranslate, type AppContextValue, type DragState } from './context';
import { dragKind, dropTargetAt, handleDrop, type PreviewRect } from './dnd';
import type { TerminalRegistry } from './terminals';
import { DockStrip } from './components/DockStrip';
import { NewSessionDialog } from './components/NewSessionDialog';
import { LayoutView } from './components/SplitView';
import { ListIcon, PlusIcon } from './components/icons';
import { SessionSidebar } from './components/SessionSidebar';
import { TakeoverDialog } from './components/TakeoverDialog';

export function App({ terminals }: { terminals: TerminalRegistry }) {
  const [view, setView] = useState<WindowView | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dragging, setDragging] = useState<DragState | null>(null);
  const [emptyDropActive, setEmptyDropActive] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
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
  useEffect(() => window.reframe.onRenameGroup(setRenaming), []);
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
    // Drags from other apps (a terminal tab torn out over us opens a new terminal window there): main uses the
    // last position as the drop point.
    let reported = 0;
    const over = (event: DragEvent) => {
      if (dragKind(event) || event.timeStamp - reported < 100) return;
      reported = event.timeStamp;
      const types = Array.from(event.dataTransfer?.types ?? []);
      window.reframe.reportForeignPointer({ x: event.screenX, y: event.screenY, types });
    };
    window.addEventListener('dragover', over, true);
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
    };
  }, []);

  const context = useMemo<AppContextValue | null>(
    () =>
      view && {
        t: makeTranslate(view.locale),
        locale: view.locale,
        windowId: view.windowId,
        sessions: view.sessions,
        terminals,
        dragging,
        setDragging,
        renaming,
        setRenaming,
      },
    [view, terminals, dragging, renaming],
  );

  if (!view || !context) return null;
  const { t } = context;

  return (
    <AppContext.Provider value={context}>
      <div className="app">
        <div className="toolbar">
          <button className="button toolbar-button" onClick={() => setDialogOpen(true)}>
            <PlusIcon />
            {t('toolbar.newSession')}
          </button>
          <button
            className={`button toolbar-button${view.sidebarOpen ? ' is-active' : ''}`}
            aria-pressed={view.sidebarOpen}
            onClick={() => window.reframe.dispatch({ type: 'window/sidebar', open: !view.sidebarOpen })}
          >
            <ListIcon />
            {t('toolbar.sessions')}
          </button>
        </div>
        <div className="workspace">
          {view.sidebarOpen && <SessionSidebar />}
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
