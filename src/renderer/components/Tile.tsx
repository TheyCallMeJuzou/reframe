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
  const tileRef = useRef<HTMLElement>(null);
  const dragSize = useRef({ width: 0, height: 0 });
  const [dropSide, setDropSide] = useState<Side | null>(null);
  const running = session && ['starting', 'idle', 'busy', 'waiting'].includes(session.status);
  const isDragged = dragging?.sessionId === sessionId;

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
    if (!isTileDrag(event) || isDragged) return;
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
    const target = { key: tileKey(sessionId), side };
    if (data.windowId === windowId) window.reframe.dispatch({ type: 'tile/move', sessionId: data.sessionId, target });
    else window.reframe.transferTile({ sessionId: data.sessionId, fromWindowId: data.windowId, target });
  };

  return (
    <section ref={tileRef} className={`tile${isDragged ? ' is-dragging' : ''}`} data-session={sessionId}>
      <header
        className="tile-header"
        draggable
        title={t('tile.dragHint')}
        onDragStart={(event) => {
          setTileDrag(event, { sessionId, windowId });
          const rect = tileRef.current?.getBoundingClientRect();
          dragSize.current = { width: rect?.width ?? 0, height: rect?.height ?? 0 };
          setDragging({ sessionId });
        }}
        onDragEnd={(event) => {
          setDragging(null);
          // No drop target took it; main tears it out if the pointer is outside every reframe window.
          if (event.dataTransfer.dropEffect === 'none') window.reframe.tearOut(sessionId, dragSize.current);
        }}
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
        {dragging && !isDragged && (
          <div className="drop-layer" onDragOver={onDragOver} onDragLeave={() => setDropSide(null)} onDrop={onDrop}>
            {dropSide && <div className={`drop-preview drop-${dropSide}`} />}
          </div>
        )}
      </div>
    </section>
  );
}
