import { useEffect, useMemo, useRef, useState, type JSX } from 'react';
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

/** Fields a job node's rendered output actually depends on (JobCard + JobNode's own
 * `selected`/`colorDisabled` props) -- anything outside this list (e.g. `needs`, which only
 * affects edges/topology) must NOT force a fresh node object on a data-only update. */
function jobSignature(job: Job, selected: boolean): string {
  return [
    job.id,
    job.title,
    job.status,
    job.owner,
    job.profile,
    job.tier ?? '',
    job.attempt,
    job.artifactCount,
    job.steeringPending,
    selected,
  ].join('|');
}

function workflowSignature(workflow: { id: string; title: string; attempt: number; ticket?: { key: string }; branch?: { name: string }; pr?: { number: number } } | undefined): string {
  if (!workflow) return 'none';
  return [
    workflow.id,
    workflow.title,
    workflow.attempt,
    workflow.ticket?.key ?? '',
    workflow.branch?.name ?? '',
    workflow.pr?.number ?? '',
  ].join('|');
}

export function Canvas({
  data,
  selectedJobId,
  onSelectJob,
  onRunJob,
}: {
  data: CpdData;
  selectedJobId?: string | null;
  onSelectJob?: (jobId: string | null) => void;
  onRunJob?: (jobId: string) => void;
}): JSX.Element {
  const [positions, setPositions] = useState<LayoutResult | null>(null);

  const graph = useMemo(() => buildGraph(data, JOB_CARD_SIZE), [data]);
  const jobById = useMemo(() => new Map(data.jobs.map((j) => [j.id, j] as const)), [data]);
  const workflowById = useMemo(
    () => new Map(data.workflows.map((wf) => [wf.id, wf] as const)),
    [data],
  );

  // L6: the graph's TOPOLOGY -- which nodes exist, their parents, and the edges between them.
  // This is deliberately independent of any job's status/title/etc, so a data-only update
  // (e.g. a job finishing, or an unrelated `messages` SSE frame causing a re-render upstream)
  // produces the same topology key and never re-triggers layout. Only an actual structural
  // change -- a node or edge added/removed/re-parented -- changes this string.
  const topologyKey = useMemo(() => {
    const nodePart = graph.nodes.map((n) => `${n.id}>${n.parentId ?? ''}`).sort().join(',');
    const edgePart = graph.edges.map((e) => `${e.source}->${e.target}`).sort().join(',');
    return `${nodePart}::${edgePart}`;
  }, [graph]);

  useEffect(() => {
    let cancelled = false;
    setPositions(null);
    elkLayout(graph).then((result) => {
      if (!cancelled) setPositions(result);
    });
    return () => {
      cancelled = true;
    };
    // Deliberately keyed on topologyKey, NOT graph: `graph` is a fresh object every time `data`
    // changes identity (every render upstream), but layout must only re-run when the topology
    // itself changed -- see topologyKey above and the Canvas falsification tests (A/C).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [topologyKey]);

  // L6: per-node object cache, keyed by node id, persisted across renders via a ref. A node's
  // object identity is reused as-is whenever its own signature (job/workflow fields + position +
  // selected) is unchanged -- this is what lets React.memo on JobNode/WorkflowHeaderNode actually
  // skip re-rendering, and is asserted directly by the Canvas falsification tests (B).
  const nodeCacheRef = useRef(new Map<string, { node: Node; signature: string }>());

  const nodes: Node[] = useMemo(() => {
    if (!positions) return [];
    const cache = nodeCacheRef.current;
    const nextCache = new Map<string, { node: Node; signature: string }>();

    const result: Node[] = [];
    for (const layoutNode of graph.nodes) {
      const pos = positions[layoutNode.id];
      if (!pos) continue;

      const job = layoutNode.jobId ? jobById.get(layoutNode.jobId) : undefined;
      let signature: string;
      let build: () => Node;

      if (job) {
        const selected = job.id === selectedJobId;
        signature = `job|${layoutNode.parentId ?? ''}|${pos.x}|${pos.y}|${jobSignature(job, selected)}`;
        build = () => ({
          id: layoutNode.id,
          type: 'job',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          data: { job, onRun: onRunJob },
          draggable: false,
          connectable: false,
          selected,
        });
      } else if (layoutNode.id.startsWith('workflow:')) {
        const workflowId = layoutNode.id.slice('workflow:'.length);
        const workflow = workflowById.get(workflowId);
        signature = `workflow|${layoutNode.parentId ?? ''}|${pos.x}|${pos.y}|${pos.width}|${pos.height}|${workflowSignature(workflow)}`;
        build = () => ({
          id: layoutNode.id,
          type: 'workflowHeader',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          style: { width: pos.width, height: pos.height },
          data: { workflow },
          draggable: false,
          connectable: false,
          selectable: false,
        });
      } else {
        // Unknown structural node: invisible, exists only so parentId-based relative
        // positioning works for its children.
        signature = `group|${layoutNode.parentId ?? ''}|${pos.x}|${pos.y}|${pos.width}|${pos.height}`;
        build = () => ({
          id: layoutNode.id,
          type: 'group',
          position: { x: pos.x, y: pos.y },
          parentId: layoutNode.parentId,
          style: { width: pos.width, height: pos.height, background: 'transparent', border: 'none' },
          data: {},
          draggable: false,
          connectable: false,
          selectable: false,
        });
      }

      const cached = cache.get(layoutNode.id);
      const node = cached && cached.signature === signature ? cached.node : build();
      nextCache.set(layoutNode.id, { node, signature });
      result.push(node);
    }

    nodeCacheRef.current = nextCache;
    return result;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graph, positions, jobById, workflowById, selectedJobId, onRunJob]);

  // L6: edges only depend on topology + per-job draft status (edgeStyle), never on data that
  // doesn't affect dash/solid -- a dedicated cache mirroring the one above keeps edge identity
  // stable across unrelated data updates too.
  const edgeCacheRef = useRef(new Map<string, { edge: Edge; signature: string }>());

  const edges: Edge[] = useMemo(() => {
    const cache = edgeCacheRef.current;
    const nextCache = new Map<string, { edge: Edge; signature: string }>();
    const result: Edge[] = [];

    for (const e of graph.edges) {
      const sourceJobId = graph.nodes.find((n) => n.id === e.source)?.jobId;
      const targetJobId = graph.nodes.find((n) => n.id === e.target)?.jobId;
      const sourceJob = sourceJobId ? jobById.get(sourceJobId) : undefined;
      const targetJob = targetJobId ? jobById.get(targetJobId) : undefined;
      const dashed = sourceJob && targetJob ? edgeStyle(sourceJob, targetJob) === 'dashed' : false;
      const signature = `${e.source}|${e.target}|${dashed}`;

      const cached = cache.get(e.id);
      const edge =
        cached && cached.signature === signature
          ? cached.edge
          : ({
              id: e.id,
              source: e.source,
              target: e.target,
              type: 'default',
              style: dashed
                ? { stroke: 'var(--line-strong)', strokeDasharray: '4 4' }
                : { stroke: 'var(--line-strong)' },
            } satisfies Edge);

      nextCache.set(e.id, { edge, signature });
      result.push(edge);
    }

    edgeCacheRef.current = nextCache;
    return result;
  }, [graph, jobById]);

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
