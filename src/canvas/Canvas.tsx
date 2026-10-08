import { useEffect, useMemo, useState } from 'react';
import {
  Background,
  Controls,
  ReactFlow,
  type Edge,
  type Node,
} from '@xyflow/react';
import type { CpdData, Job } from '../model/types';
import type { LayoutResult } from '../layout/types';
import { buildGraph } from '../layout/toGraph';
import { elkLayout } from '../layout/elk';
import { JobNode } from './nodes/JobNode';
import { WorkflowHeaderNode } from './nodes/WorkflowHeaderNode';
import { edgeStyle } from './edges';

const JOB_CARD_SIZE = { width: 220, height: 96 };

const nodeTypes = {
  job: JobNode,
  workflowHeader: WorkflowHeaderNode,
};

export function Canvas({
  data,
  selectedJobId,
  onSelectJob,
}: {
  data: CpdData;
  selectedJobId?: string | null;
  onSelectJob?: (jobId: string | null) => void;
}): JSX.Element {
  const [positions, setPositions] = useState<LayoutResult | null>(null);

  const graph = useMemo(() => buildGraph(data, JOB_CARD_SIZE), [data]);
  const jobById = useMemo(() => new Map(data.jobs.map((j) => [j.id, j] as const)), [data]);
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

      const job = layoutNode.jobId ? jobById.get(layoutNode.jobId) : undefined;
      if (job) {
        return {
          id: layoutNode.id,
          type: 'job',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          data: { job },
          draggable: false,
          connectable: false,
          selected: job.id === selectedJobId,
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
    const sourceJobId = graph.nodes.find((n) => n.id === e.source)?.jobId;
    const targetJobId = graph.nodes.find((n) => n.id === e.target)?.jobId;
    const sourceJob = sourceJobId ? jobById.get(sourceJobId) : undefined;
    const targetJob = targetJobId ? jobById.get(targetJobId) : undefined;
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
      elementsSelectable={false}
      onNodeClick={(_, node) => {
        if (node.type === 'job') onSelectJob?.((node.data.job as Job).id);
      }}
      onPaneClick={() => onSelectJob?.(null)}
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
