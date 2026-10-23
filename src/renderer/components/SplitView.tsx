import { Fragment, useEffect, useMemo } from 'react';
import { Group, Panel, Separator, useGroupRef, type Layout } from 'react-resizable-panels';
import { nodeKey, type LayoutNode, type SplitNode } from '@shared/layout';
import { Tile } from './Tile';

export function LayoutView({ node, windowId }: { node: LayoutNode; windowId: string }) {
  if (node.kind === 'tile') return <Tile sessionId={node.sessionId} windowId={windowId} />;
  if (node.kind === 'group') return <LayoutView node={node.child} windowId={windowId} />;
  return <SplitView split={node} windowId={windowId} />;
}

function SplitView({ split, windowId }: { split: SplitNode; windowId: string }) {
  const groupRef = useGroupRef();
  const keys = useMemo(() => split.children.map(nodeKey), [split.children]);
  const layout = useMemo<Layout>(
    () => Object.fromEntries(keys.map((key, i) => [key, split.sizes[i] ?? 100 / keys.length])),
    [keys, split.sizes],
  );

  // Sizes can change from outside (a tile added or restored); apply them unless they already match.
  useEffect(() => {
    const group = groupRef.current;
    if (!group) return;
    const current = group.getLayout();
    const differs = Object.entries(layout).some(([key, size]) => Math.abs((current[key] ?? -1) - size) > 0.5);
    if (differs) group.setLayout(layout);
  }, [groupRef, layout]);

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
            <LayoutView node={child} windowId={windowId} />
          </Panel>
        </Fragment>
      ))}
    </Group>
  );
}
