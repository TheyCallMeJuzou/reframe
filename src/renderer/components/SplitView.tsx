import { Fragment, useEffect, useMemo } from 'react';
import { Group, Panel, Separator, useGroupRef, type Layout } from 'react-resizable-panels';
import { nodeKey, type GroupNode, type LayoutNode, type SplitNode } from '@shared/layout';
import { GroupFrame } from './GroupFrame';
import { Tile } from './Tile';

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
  const groupRef = useGroupRef();
  const keys = useMemo(() => split.children.map(nodeKey), [split.children]);
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
      className="split"
      orientation={split.dir === 'row' ? 'horizontal' : 'vertical'}
      defaultLayout={layout}
      onLayoutChanged={(next, meta) => {
        if (!meta.isUserInteraction) return;
        window.reframe.dispatch({ type: 'split/resize', splitId: split.id, sizes: keys.map((k) => next[k] ?? 0) });
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
