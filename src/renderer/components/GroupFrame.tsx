import { useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { leaves, type GroupNode, type Side } from '@shared/layout';
import { useApp } from '../context';
import { dragKind, handleDrop, setNodeDrag } from '../dnd';
import { CollapseIcon, GripIcon, MoreIcon } from './icons';
import { NameInput } from './NameInput';

const SIDES: Side[] = ['left', 'right', 'top', 'bottom'];

/** A colored frame with a bar around a group's tiles. Its edges take drops that go next to the group. */
export function GroupFrame({ group, children }: { group: GroupNode; children: ReactNode }) {
  const { t, windowId, dragging, setDragging, renaming, setRenaming } = useApp();
  const frameRef = useRef<HTMLDivElement>(null);
  const dragSize = useRef({ width: 0, height: 0 });
  const [band, setBand] = useState<Side | null>(null);
  const count = leaves(group).length;
  const isDragged = dragging?.key === group.id;

  return (
    <div
      ref={frameRef}
      className={`group${isDragged ? ' is-dragging' : ''}`}
      style={{ '--group-color': group.color } as CSSProperties}
      data-group={group.id}
    >
      <div
        className="group-header"
        draggable={renaming !== group.id}
        title={t('group.dragHint')}
        onContextMenu={(event) => {
          event.preventDefault();
          window.reframe.showGroupMenu(group.id);
        }}
        onDragStart={(event) => {
          setNodeDrag(event, 'group', { key: group.id, windowId });
          const rect = frameRef.current?.getBoundingClientRect();
          dragSize.current = { width: rect?.width ?? 0, height: rect?.height ?? 0 };
          setDragging({ key: group.id, kind: 'group' });
        }}
        onDragEnd={(event) => {
          setDragging(null);
          if (event.dataTransfer.dropEffect === 'none') window.reframe.tearOut(group.id, dragSize.current);
        }}
      >
        <button
          className="icon-button"
          title={t('group.collapse')}
          aria-label={t('group.collapse')}
          onClick={() => window.reframe.dispatch({ type: 'group/collapse', groupId: group.id })}
        >
          <CollapseIcon />
        </button>
        <span className="tile-grip" aria-hidden="true">
          <GripIcon />
        </span>
        <button
          className="group-color"
          title={t('group.color')}
          aria-label={t('group.color')}
          onClick={() => window.reframe.showGroupColorMenu(group.id)}
        />
        {renaming === group.id ? (
          <NameInput
            className="group-name-input"
            initial={group.name}
            onCommit={(name) => name && window.reframe.dispatch({ type: 'group/rename', groupId: group.id, name })}
            onDone={() => setRenaming(null)}
          />
        ) : (
          <span className="group-name" title={t('group.renameHint')} onDoubleClick={() => setRenaming(group.id)}>
            {group.name}
          </span>
        )}
        <span className="group-count">{count}</span>
        <span className="spacer" />
        <button
          className="icon-button"
          title={t('group.options')}
          aria-label={t('group.options')}
          onClick={() => window.reframe.showGroupMenu(group.id)}
        >
          <MoreIcon />
        </button>
      </div>
      <div className="group-body">{children}</div>
      {dragging &&
        !isDragged &&
        SIDES.map((side) => (
          <div
            key={side}
            className={`group-band band-${side}`}
            onDragOver={(event) => {
              if (!dragKind(event)) return;
              event.preventDefault();
              event.dataTransfer.dropEffect = 'move';
              setBand(side);
            }}
            onDragLeave={() => setBand((current) => (current === side ? null : current))}
            onDrop={(event) => {
              setBand(null);
              if (handleDrop(event, windowId, { key: group.id, side })) event.preventDefault();
            }}
          />
        ))}
      {dragging && band && <div className={`drop-preview drop-${band}`} />}
    </div>
  );
}
