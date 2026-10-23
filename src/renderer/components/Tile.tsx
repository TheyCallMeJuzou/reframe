import { useEffect, useLayoutEffect, useRef, useState, type DragEvent } from 'react';
import type { Side } from '@shared/layout';
import { tileKey } from '@shared/layout';
import { useApp } from '../context';
import { isTileDrag, readTileDrag, setTileDrag, sideAt } from '../dnd';
import { CloseIcon, CollapseIcon, GripIcon } from './icons';
import { StatusDot } from './StatusDot';

export function Tile({ sessionId, windowId }: { sessionId: string; windowId: string }) {
  const { t, sessions, terminals, dragging, setDragging } = useApp();
  const session = sessions[sessionId];
  const slot = useRef<HTMLDivElement>(null);
  const [dropSide, setDropSide] = useState<Side | null>(null);
  const running = session && ['starting', 'idle', 'busy', 'waiting'].includes(session.status);

  useLayoutEffect(() => {
    const el = slot.current;
    if (!el) return;
    terminals.mount(sessionId, el);
    return () => terminals.unmount(sessionId, el);
  }, [sessionId, terminals]);

  // A restarted session has a new process; drop the old screen and connect again.
  const previous = useRef(session?.status);
  useEffect(() => {
    const was = previous.current;
    previous.current = session?.status;
    if ((was === 'ended' || was === 'error') && session?.status === 'starting') terminals.reattach(sessionId);
  }, [session?.status, sessionId, terminals]);

  if (!session) return null;

  const onDragOver = (event: DragEvent) => {
    if (!isTileDrag(event) || dragging === sessionId) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    setDropSide(sideAt(event.currentTarget.getBoundingClientRect(), event.clientX, event.clientY));
  };
  const onDrop = (event: DragEvent) => {
    const data = readTileDrag(event);
    const side = dropSide;
    setDropSide(null);
    if (!data || !side || data.sessionId === sessionId) return;
    event.preventDefault();
    window.reframe.dispatch({
      type: 'tile/move',
      sessionId: data.sessionId,
      target: { key: tileKey(sessionId), side },
    });
  };

  return (
    <section className={`tile${dragging === sessionId ? ' is-dragging' : ''}`} data-session={sessionId}>
      <header
        className="tile-header"
        draggable
        title={t('tile.dragHint')}
        onDragStart={(event) => {
          setTileDrag(event, { sessionId, windowId });
          setDragging(sessionId);
        }}
        onDragEnd={() => setDragging(null)}
      >
        <button
          className="icon-button"
          title={t('tile.collapse')}
          aria-label={t('tile.collapse')}
          onClick={() => window.reframe.dispatch({ type: 'tile/collapse', sessionId })}
        >
          <CollapseIcon />
        </button>
        <span className="tile-grip" aria-hidden="true">
          <GripIcon />
        </span>
        <StatusDot status={session.status} />
        <span className="tile-label">{session.label}</span>
        <span className="tile-cwd" title={session.cwd}>
          {session.cwd}
        </span>
        <button
          className="icon-button"
          title={t('tile.end')}
          aria-label={t('tile.end')}
          onClick={() => window.reframe.endSession(sessionId)}
        >
          <CloseIcon />
        </button>
      </header>
      <div className="tile-body" onMouseDown={() => terminals.focus(sessionId)}>
        <div className="tile-terminal" ref={slot} />
        {!running && (
          <div className="tile-overlay">
            <div className="tile-overlay-card">
              <p>{session.error ? t(`tile.error.${session.error}`) : t('tile.ended')}</p>
              <div className="button-row">
                {session.error === 'transcriptMissing' || session.error === 'folderMissing' ? null : (
                  <button className="button primary" onClick={() => window.reframe.restartSession(sessionId)}>
                    {t('tile.resume')}
                  </button>
                )}
                {session.error && session.error !== 'folderMissing' && (
                  <button className="button" onClick={() => window.reframe.restartSession(sessionId, true)}>
                    {t('tile.startFresh')}
                  </button>
                )}
                <button className="button" onClick={() => window.reframe.endSession(sessionId)}>
                  {t('tile.close')}
                </button>
              </div>
            </div>
          </div>
        )}
        {dragging !== null && dragging !== sessionId && (
          <div className="drop-layer" onDragOver={onDragOver} onDragLeave={() => setDropSide(null)} onDrop={onDrop}>
            {dropSide && <div className={`drop-preview drop-${dropSide}`} />}
          </div>
        )}
      </div>
    </section>
  );
}
