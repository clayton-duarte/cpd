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
import { SessionFrameNode } from './nodes/SessionFrameNode';
import { WorkflowHeaderNode } from './nodes/WorkflowHeaderNode';
import { WaveGroupNode } from './nodes/WaveGroupNode';
import { LeadNode } from './nodes/LeadNode';
import { edgeStyle } from './edges';

const JOB_CARD_SIZE = { width: 220, height: 96 };

const nodeTypes = {
  job: JobNode,
  sessionFrame: SessionFrameNode,
  workflowHeader: WorkflowHeaderNode,
  waveGroup: WaveGroupNode,
  lead: LeadNode,
};

export function Canvas({ data }: { data: CpdData }): JSX.Element {
  const [positions, setPositions] = useState<LayoutResult | null>(null);

  const graph = useMemo(() => buildGraph(data, JOB_CARD_SIZE), [data]);
  const jobById = useMemo(() => new Map(data.jobs.map((j) => [j.id, j] as const)), [data]);
  const sessionById = useMemo(
    () => new Map(data.sessions.map((s) => [s.id, s] as const)),
    [data],
  );
  const workflowById = useMemo(
    () => new Map(data.workflows.map((wf) => [wf.id, wf] as const)),
    [data],
  );

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
          color: 'var(--fg-faint)',
        }}
      >
        Laying out…
      </div>
    );
  }

  const nodes: Node[] = graph.nodes
    .map((layoutNode): Node | null => {
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
        };
      }

      if (layoutNode.id.startsWith('session:')) {
        const sessionId = layoutNode.id.slice('session:'.length);
        const session = sessionById.get(sessionId);
        return {
          id: layoutNode.id,
          type: 'sessionFrame',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          style: { width: pos.width, height: pos.height },
          data: { session },
          draggable: false,
          connectable: false,
          selectable: false,
        };
      }

      if (layoutNode.id.startsWith('lead:')) {
        const sessionId = layoutNode.id.slice('lead:'.length);
        const session = sessionById.get(sessionId);
        if (!session) return null;
        return {
          id: layoutNode.id,
          type: 'lead',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          data: { session },
          draggable: false,
          connectable: false,
          selectable: false,
        };
      }

      if (layoutNode.id.startsWith('workflow:')) {
        const workflowId = layoutNode.id.slice('workflow:'.length);
        const workflow = workflowById.get(workflowId);
        return {
          id: layoutNode.id,
          type: 'workflowHeader',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          style: { width: pos.width, height: pos.height },
          data: { workflow },
          draggable: false,
          connectable: false,
          selectable: false,
        };
      }

      if (layoutNode.id.startsWith('wave:')) {
        return {
          id: layoutNode.id,
          type: 'waveGroup',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          style: { width: pos.width, height: pos.height },
          data: {},
          draggable: false,
          connectable: false,
          selectable: false,
        };
      }

      // Unknown structural node: invisible, exists only so parentId-based
      // relative positioning works for its children.
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
      };
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
      style: { stroke: 'var(--line-strong)' },
      type: 'default',
      ...(dashed ? { style: { stroke: 'var(--line-strong)', strokeDasharray: '4 4' } } : {}),
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
      <Background color="var(--line)" />
      <Controls />
    </ReactFlow>
  );
}
