import { useEffect, useLayoutEffect, useRef, useState, type DragEvent } from 'react';
import { tileKey, type GroupNode, type Side } from '@shared/layout';
import { useApp } from '../context';
import { dragKind, handleDrop, setNodeDrag, sideAt } from '../dnd';
import { CloseIcon, CollapseIcon, GripIcon } from './icons';
import { NameInput } from './NameInput';
import { StatusDot } from './StatusDot';

/** A session's tile; `group` is the group it sits in, if any. */
export function Tile({ sessionId, group }: { sessionId: string; group: GroupNode | null }) {
  const { t, keys, windowId, sessions, terminals, dragging, setDragging, renamingSession, setRenamingSession } =
    useApp();
  const session = sessions[sessionId];
  const slot = useRef<HTMLDivElement>(null);
  const tileRef = useRef<HTMLElement>(null);
  const dragSize = useRef({ width: 0, height: 0 });
  const [dropSide, setDropSide] = useState<Side | null>(null);
  const [filesOver, setFilesOver] = useState(false);
  const running = session && ['starting', 'idle', 'busy', 'waiting'].includes(session.status);
  const key = tileKey(sessionId);
  const isDragged = dragging?.key === key || (!!group && dragging?.key === group.id);
  // A dragged group never lands inside another group; the group's edge bands take that drop instead.
  const acceptsDrop = !!dragging && !isDragged && !(dragging.kind === 'group' && group);

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
  const editingName = renamingSession?.sessionId === sessionId && renamingSession.place === 'tile';

  const onDragOver = (event: DragEvent) => {
    if (!dragKind(event) || !acceptsDrop) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    setDropSide(sideAt(event.currentTarget.getBoundingClientRect(), event.clientX, event.clientY));
  };
  // Files dropped onto a running session are typed in as their paths, like in a terminal.
  const carriesFiles = (event: DragEvent) => !dragKind(event) && Array.from(event.dataTransfer.types).includes('Files');
  const onFilesOver = (event: DragEvent) => {
    if (!running || !carriesFiles(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    setFilesOver(true);
  };
  const onFilesDrop = (event: DragEvent) => {
    setFilesOver(false);
    if (!running || !carriesFiles(event)) return;
    event.preventDefault();
    const files = Array.from(event.dataTransfer.files);
    void Promise.all(
      files.map(
        async (file) =>
          window.reframe.pathForFile(file) ||
          (await window.reframe.saveDroppedFile(file.name, await file.arrayBuffer())),
      ),
    ).then((paths) =>
      terminals.pastePaths(
        sessionId,
        paths.filter((p): p is string => !!p),
      ),
    );
  };

  const onDrop = (event: DragEvent) => {
    // Taken from the drop itself: the state set by dragover may not have rendered yet on a quick drop.
    const side = sideAt(event.currentTarget.getBoundingClientRect(), event.clientX, event.clientY);
    setDropSide(null);
    if (handleDrop(event, windowId, { key, side })) event.preventDefault();
  };

  return (
    <section ref={tileRef} className={`tile${isDragged ? ' is-dragging' : ''}`} data-session={sessionId}>
      <header
        className="tile-header"
        draggable={!editingName}
        title={t('tile.dragHint')}
        onContextMenu={(event) => {
          event.preventDefault();
          window.reframe.showTileMenu(sessionId);
        }}
        onDragStart={(event) => {
          event.stopPropagation();
          setNodeDrag(event, 'tile', { key, windowId });
          const rect = tileRef.current?.getBoundingClientRect();
          dragSize.current = { width: rect?.width ?? 0, height: rect?.height ?? 0 };
          setDragging({ key, kind: 'tile' });
        }}
        onDragEnd={(event) => {
          setDragging(null);
          // No drop target took it; main tears it out if the pointer is outside every reframe window.
          if (event.dataTransfer.dropEffect === 'none') window.reframe.tearOut(key, dragSize.current);
        }}
      >
        <button
          className="icon-button"
          title={`${t('tile.collapse')} (${keys('M')})`}
          aria-label={t('tile.collapse')}
          onClick={() => window.reframe.dispatch({ type: 'tile/collapse', sessionId })}
        >
          <CollapseIcon />
        </button>
        <span className="tile-grip" aria-hidden="true">
          <GripIcon />
        </span>
        <StatusDot status={session.status} />
        {editingName ? (
          <NameInput
            className="tile-label-input"
            initial={session.label}
            placeholder={t('session.automaticName')}
            onCommit={(name) => window.reframe.renameSession(sessionId, name)}
            onDone={() => {
              setRenamingSession(null);
              // Back to typing in the session, once the field is gone.
              requestAnimationFrame(() => terminals.focus(sessionId));
            }}
          />
        ) : (
          <span
            className="tile-label"
            title={t('session.renameHint')}
            onDoubleClick={() => setRenamingSession({ sessionId, place: 'tile' })}
          >
            {session.label}
          </span>
        )}
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
      <div
        className="tile-body"
        onMouseDown={() => terminals.focus(sessionId)}
        onDragOver={onFilesOver}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFilesOver(false);
        }}
        onDrop={onFilesDrop}
      >
        {filesOver && <div className="files-drop">{t('tile.dropFiles')}</div>}
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
        {acceptsDrop && (
          <div className="drop-layer" onDragOver={onDragOver} onDragLeave={() => setDropSide(null)} onDrop={onDrop}>
            {dropSide && <div className={`drop-preview drop-${dropSide}`} />}
          </div>
        )}
      </div>
    </section>
  );
}
