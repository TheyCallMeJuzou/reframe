import { useEffect, useMemo, useState } from 'react';
import type { WindowView } from '@shared/ipc';
import { AppContext, makeTranslate, type AppContextValue } from './context';
import type { TerminalRegistry } from './terminals';
import { DockStrip } from './components/DockStrip';
import { NewSessionDialog } from './components/NewSessionDialog';
import { LayoutView } from './components/SplitView';
import { PlusIcon } from './components/icons';

export function App({ terminals }: { terminals: TerminalRegistry }) {
  const [view, setView] = useState<WindowView | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dragging, setDragging] = useState<string | null>(null);

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

  // A drop re-parents the dragged tile, so its own dragend may never reach React; reset here too.
  useEffect(() => {
    const reset = () => setDragging(null);
    window.addEventListener('drop', reset, true);
    window.addEventListener('dragend', reset, true);
    return () => {
      window.removeEventListener('drop', reset, true);
      window.removeEventListener('dragend', reset, true);
    };
  }, []);

  const context = useMemo<AppContextValue | null>(
    () =>
      view && {
        t: makeTranslate(view.locale),
        sessions: view.sessions,
        terminals,
        dragging,
        setDragging,
      },
    [view, terminals, dragging],
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
        </div>
        <div className="workspace">
          <DockStrip entries={view.dock} />
          <main className="layout">
            {view.layout ? (
              <LayoutView node={view.layout} windowId={view.windowId} />
            ) : (
              <div className="empty">
                <h1>{t('empty.title')}</h1>
                <p>{t('empty.hint')}</p>
                <button className="button primary" onClick={() => setDialogOpen(true)}>
                  <PlusIcon />
                  {t('toolbar.newSession')}
                </button>
              </div>
            )}
          </main>
        </div>
        {dialogOpen && <NewSessionDialog onClose={() => setDialogOpen(false)} />}
      </div>
    </AppContext.Provider>
  );
}
