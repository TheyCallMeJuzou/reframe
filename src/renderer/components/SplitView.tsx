import { Fragment, useEffect, useMemo, useRef } from 'react';
import { Group, Panel, Separator, useGroupRef, type Layout } from 'react-resizable-panels';
import { nodeKey, type GroupNode, type LayoutNode, type SplitNode } from '@shared/layout';
import { useApp } from '../context';
import { GRID_STEPS, snapSizes } from '../snap';
import { GroupFrame } from './GroupFrame';
import { Tile } from './Tile';

/**
 * A separator being dragged with the pointer (snapping is for that, not for resizing with the keyboard), and
 * whether Alt is held, which drags freely. Alt released after such a drag does not open the window's menu bar.
 */
const separatorDrag = { active: false, free: false, altUsed: false };
window.addEventListener(
  'pointerdown',
  (event) => {
    separatorDrag.active = !!(event.target as Element | null)?.closest?.('.separator');
    separatorDrag.free = event.altKey;
  },
  true,
);
window.addEventListener(
  'pointermove',
  (event) => {
    separatorDrag.free = event.altKey;
    if (separatorDrag.active && event.altKey) separatorDrag.altUsed = true;
  },
  true,
);
// After the panel library had its say about the release (which ends the drag).
window.addEventListener('pointerup', () => setTimeout(() => (separatorDrag.active = false)), true);
window.addEventListener(
  'keyup',
  (event) => {
    if (event.key !== 'Alt' || !separatorDrag.altUsed) return;
    separatorDrag.altUsed = false;
    event.preventDefault();
  },
  true,
);

/** Renders a layout node; `group` is the group the node sits in (groups do not nest). */
export function LayoutView({ node, group = null }: { node: LayoutNode; group?: GroupNode | null }) {
  if (node.kind === 'tile') return <Tile sessionId={node.sessionId} group={group} />;
  if (node.kind === 'group') {
    return (
      <GroupFrame group={node}>
        <LayoutView node={node.child} group={node} />
      </GroupFrame>
    );
  }
  // The panel library cannot change a group's set of panels and its layout in one step, so a split whose
  // children change is rebuilt with a fresh default layout. Terminals survive this (see TerminalRegistry).
  return <SplitView key={`${node.id}:${node.children.map(nodeKey).join('|')}`} split={node} group={group} />;
}

function SplitView({ split, group }: { split: SplitNode; group: GroupNode | null }) {
  const { snapToGrid } = useApp();
  const groupRef = useGroupRef();
  const elementRef = useRef<HTMLDivElement>(null);
  const keys = useMemo(() => split.children.map(nodeKey), [split.children]);

  const sizesOf = (next: Layout) => keys.map((key) => next[key] ?? 0);
  const snapping = () => snapToGrid && separatorDrag.active && !separatorDrag.free;

  /** `sizes` with a separator that moved since the drag began on the grid over the whole layout. */
  const onGrid = (sizes: number[]): number[] => {
    const element = elementRef.current;
    const area = element?.closest('.layout');
    if (!element || !area) return sizes;
    const across = split.dir === 'row';
    const box = element.getBoundingClientRect();
    const grid = area.getBoundingClientRect();
    const separator = element.querySelector(':scope > .separator')?.getBoundingClientRect();
    return snapSizes(
      sizes,
      split.sizes, // as they were before the drag
      {
        start: across ? box.left : box.top,
        length: across ? box.width : box.height,
        separator: separator ? (across ? separator.width : separator.height) : 0,
      },
      { start: across ? grid.left : grid.top, length: across ? grid.width : grid.height, steps: GRID_STEPS },
    );
  };

  // A drag of one of this split's separators shows its sizes on the grid as it goes (the panel library works each
  // one out from where the drag began), and saves the last of them on release: having been set here, the release
  // no longer counts as the user's change for the library.
  const dragged = useRef<number[] | null>(null);
  const onLayoutChange = (next: Layout) => {
    if (!separatorDrag.active) return;
    const sizes = sizesOf(next);
    const shown = snapping() ? onGrid(sizes) : sizes;
    dragged.current = shown;
    if (shown.some((size, i) => Math.abs(size - sizes[i]!) > 0.05)) {
      groupRef.current?.setLayout(Object.fromEntries(keys.map((key, i) => [key, shown[i]!])));
    }
  };
  useEffect(() => {
    const release = () => {
      const sizes = dragged.current;
      dragged.current = null;
      if (sizes) window.reframe.dispatch({ type: 'split/resize', splitId: split.id, sizes });
    };
    window.addEventListener('pointerup', release);
    return () => window.removeEventListener('pointerup', release);
  }, [split.id]);
  const layout = useMemo<Layout>(
    () => Object.fromEntries(keys.map((key, i) => [key, split.sizes[i] ?? 100 / keys.length])),
    [keys, split.sizes],
  );

  // Sizes of the same panels can still change from outside; apply them unless they already match.
  useEffect(() => {
    const panels = groupRef.current;
    if (!panels) return;
    const current = panels.getLayout();
    if (Object.keys(current).length !== keys.length) return;
    const differs = Object.entries(layout).some(([key, size]) => Math.abs((current[key] ?? -1) - size) > 0.5);
    if (differs) panels.setLayout(layout);
  }, [groupRef, keys, layout]);

  return (
    <Group
      id={split.id}
      groupRef={groupRef}
      elementRef={elementRef}
      className="split"
      orientation={split.dir === 'row' ? 'horizontal' : 'vertical'}
      defaultLayout={layout}
      // While dragging, the separator jumps from grid line to grid line.
      onLayoutChange={onLayoutChange}
      onLayoutChanged={(next, meta) => {
        // A pointer drag is saved on release (above); this is for resizing with the keyboard.
        if (!meta.isUserInteraction || dragged.current) return;
        window.reframe.dispatch({ type: 'split/resize', splitId: split.id, sizes: sizesOf(next) });
      }}
    >
      {split.children.map((child, i) => (
        <Fragment key={keys[i]}>
          {i > 0 && <Separator className={`separator separator-${split.dir}`} />}
          <Panel id={keys[i]} minSize={split.dir === 'row' ? 160 : 90} className="panel">
            <LayoutView node={child} group={group} />
          </Panel>
        </Fragment>
      ))}
    </Group>
  );
}
