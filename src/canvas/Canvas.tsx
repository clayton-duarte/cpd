import { useEffect, useMemo, useState } from 'react';
import {
  Background,
  Controls,
  ReactFlow,
  type Edge,
  type Node,
} from '@xyflow/react';
import type { CpdData } from '../model/types';
import type { LayoutResult } from '../layout/types';
import { buildGraph } from '../layout/toGraph';
import { elkLayout } from '../layout/elk';
import { JobNode } from './nodes/JobNode';
import { edgeStyle } from './edges';
import { w } from '../theme';

const JOB_CARD_SIZE = { width: 220, height: 96 };

const nodeTypes = { job: JobNode };

export function Canvas({ data }: { data: CpdData }): JSX.Element {
  const [positions, setPositions] = useState<LayoutResult | null>(null);

  const graph = useMemo(() => buildGraph(data, JOB_CARD_SIZE), [data]);
  const jobById = useMemo(() => new Map(data.jobs.map((j) => [j.id, j] as const)), [data]);

  useEffect(() => {
    let cancelled = false;
    setPositions(null);
    elkLayout(graph).then((result) => {
      if (!cancelled) setPositions(result);
    });
    return () => {
      cancelled = true;
    };
  }, [graph]);

  if (!positions) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          color: w(0.45),
        }}
      >
        Laying out…
      </div>
    );
  }

  const nodes: Node[] = graph.nodes
    .map((layoutNode) => {
      const pos = positions[layoutNode.id];
      if (!pos) return null;
      const job = jobById.get(layoutNode.id);
      if (job) {
        return {
          id: layoutNode.id,
          type: 'job',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          data: { job },
          draggable: false,
          connectable: false,
        } satisfies Node;
      }
      // Structural (session/workflow/wave) container node: invisible, exists
      // only so parentId-based relative positioning works for its children.
      return {
        id: layoutNode.id,
        type: 'group',
        position: { x: pos.x, y: pos.y },
        parentId: layoutNode.parentId,
        style: { width: pos.width, height: pos.height, background: 'transparent', border: 'none' },
        data: {},
        draggable: false,
        connectable: false,
        selectable: false,
      } satisfies Node;
    })
    .filter((n): n is Node => n !== null);

  const edges: Edge[] = graph.edges.map((e) => {
    const sourceJob = jobById.get(e.source);
    const targetJob = jobById.get(e.target);
    const dashed = sourceJob && targetJob ? edgeStyle(sourceJob, targetJob) === 'dashed' : false;
    return {
      id: e.id,
      source: e.source,
      target: e.target,
      style: { stroke: w(0.25) },
      type: 'default',
      ...(dashed ? { style: { stroke: w(0.25), strokeDasharray: '4 4' } } : {}),
    } satisfies Edge;
  });

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      nodesDraggable={false}
      nodesConnectable={false}
      panOnDrag
      zoomOnScroll
      fitView
      onlyRenderVisibleElements
    >
      <Background color={w(0.08)} />
      <Controls />
    </ReactFlow>
  );
}
