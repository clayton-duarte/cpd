import type { JobView, WorkflowView } from "../../../protocol/cockpit.ts";
import { parseJobStatus } from "../../dango-status.ts";

export const NODE_W = 148;
export const NODE_H = 46;
export const GAP_X = 28;
export const GAP_Y = 8;
const COL_STEP = NODE_W + GAP_X;
const ROW_STEP = NODE_H + GAP_Y;

export interface DagNode { readonly job: JobView; readonly col: number; readonly left: number; readonly top: number }
export interface DagEdge { readonly id: string; readonly d: string; readonly state: "done" | "pending" }
export interface DagLayout { readonly width: number; readonly height: number; readonly nodes: readonly DagNode[]; readonly edges: readonly DagEdge[] }

export interface WorkflowEdge {
  readonly fromKey: string;
  readonly toKey: string;
  readonly kind: "base" | "needs";
}

export interface LaneAllocation {
  readonly lanes: readonly number[];
  readonly laneCount: number;
}

export function allocateLanes(
  order: readonly WorkflowView[],
  edges: readonly WorkflowEdge[],
): LaneAllocation {
  if (edges.length === 0) {
    return { lanes: [], laneCount: 0 };
  }

  const indexMap = new Map<string, number>();
  order.forEach((wf, idx) => {
    indexMap.set(wf.key, idx);
  });

  // Edge with its original index, source index, target index
  type EdgeEntry = {
    origIdx: number;
    iu: number;
    id: number;
  };

  const edgeEntries: EdgeEntry[] = edges.map((e, idx) => {
    const iu = indexMap.get(e.fromKey) ?? 0;
    const id = indexMap.get(e.toKey) ?? 0;
    return {
      origIdx: idx,
      iu: Math.min(iu, id),
      id: Math.max(iu, id),
    };
  });

  // Sort edges by (iu, id) ascending, ties by origIdx
  const sorted = [...edgeEntries].sort((a, b) => {
    if (a.iu !== b.iu) return a.iu - b.iu;
    if (a.id !== b.id) return a.id - b.id;
    return a.origIdx - b.origIdx;
  });

  // Assigned lane per original edge index
  const assignedLanes = new Array<number>(edges.length);
  // Placed edges per lane: list of [iu, id] intervals
  const lanesEdges: Array<Array<{ iu: number; id: number }>> = [];

  for (const entry of sorted) {
    let lane = 0;
    while (true) {
      if (lane >= lanesEdges.length) {
        lanesEdges.push([]);
        break;
      }
      // Check conflict with already-placed edges in this lane
      // Conflict: closed spans intersect [iu1, id1] and [iu2, id2] -> !(id1 < iu2 || id2 < iu1)
      const hasConflict = lanesEdges[lane]!.some(
        (span) => !(entry.id < span.iu || entry.iu > span.id),
      );
      if (!hasConflict) {
        break;
      }
      lane++;
    }
    lanesEdges[lane]!.push({ iu: entry.iu, id: entry.id });
    assignedLanes[entry.origIdx] = lane;
  }

  return {
    lanes: assignedLanes,
    laneCount: lanesEdges.length,
  };
}

export function computeWorkflowEdges(planId: string, workflows: readonly WorkflowView[]): WorkflowEdge[] {
  const byKey = new Map(workflows.map((w) => [w.key, w]));
  const edges: WorkflowEdge[] = [];

  for (const d of workflows) {
    if (d.base_workflow != null) {
      const prefix = `${planId}/`;
      if (d.base_workflow.startsWith(prefix)) {
        const upstreamKey = d.base_workflow.slice(prefix.length);
        if (byKey.has(upstreamKey)) {
          edges.push({
            fromKey: upstreamKey,
            toKey: d.key,
            kind: "base",
          });
        }
      }
    }
    for (const needKey of d.needs) {
      if (byKey.has(needKey)) {
        edges.push({
          fromKey: needKey,
          toKey: d.key,
          kind: "needs",
        });
      }
    }
  }

  return edges;
}

export function topoOrder(workflows: readonly WorkflowView[], edges: readonly WorkflowEdge[], planId: string): WorkflowView[] {
  // Upstream must precede dependant.
  // At each step, emit the earliest card in workflows order whose upstreams are all emitted.
  // Upstreams of d are fromKey of edges where toKey === d.key.
  const upstreamsByTarget = new Map<string, Set<string>>();
  for (const w of workflows) {
    upstreamsByTarget.set(w.key, new Set());
  }
  for (const e of edges) {
    upstreamsByTarget.get(e.toKey)?.add(e.fromKey);
  }

  const emitted = new Set<string>();
  const remaining = [...workflows];
  const result: WorkflowView[] = [];

  while (remaining.length > 0) {
    const readyIdx = remaining.findIndex((w) => {
      const deps = upstreamsByTarget.get(w.key)!;
      for (const u of deps) {
        if (!emitted.has(u)) return false;
      }
      return true;
    });

    if (readyIdx === -1) {
      // Cycle detected
      throw new Error(`plan ${planId} has a workflow dependency cycle at ${remaining[0]!.key}`);
    }

    const readyWf = remaining.splice(readyIdx, 1)[0]!;
    emitted.add(readyWf.key);
    result.push(readyWf);
  }

  return result;
}

export function layoutDag(wf: WorkflowView, jobs: readonly JobView[]): DagLayout {
  const members = wf.jobKeys.map((key) => {
    const job = jobs.find((j) => j.key === key);
    if (job === undefined) throw new Error(`workflow ${wf.key} lists unknown job ${key}`);
    return job;
  });
  if (members.length === 0) return { width: 0, height: 0, nodes: [], edges: [] };
  const inWf = new Set(wf.jobKeys);
  const byKey = new Map(members.map((j) => [j.key, j]));
  const cols = new Map<string, number>();
  const visiting = new Set<string>();
  const colOf = (job: JobView): number => {
    const known = cols.get(job.key);
    if (known !== undefined) return known;
    if (visiting.has(job.key)) throw new Error(`workflow ${wf.key} has a dependency cycle at ${job.key}`);
    visiting.add(job.key);
    let col = 0;
    for (const n of job.needs) {
      const dep = inWf.has(n) ? byKey.get(n) : undefined;
      if (dep !== undefined) col = Math.max(col, 1 + colOf(dep));
    }
    visiting.delete(job.key);
    cols.set(job.key, col);
    return col;
  };
  for (const j of members) colOf(j);
  const ncols = Math.max(...cols.values()) + 1;
  const sizes: number[] = Array.from({ length: ncols }, () => 0);
  const rows = new Map<string, number>();
  for (const j of members) {
    const c = cols.get(j.key) as number;
    rows.set(j.key, sizes[c] as number);
    sizes[c] = (sizes[c] as number) + 1;
  }
  const colH = (c: number): number => (sizes[c] as number) * NODE_H + ((sizes[c] as number) - 1) * GAP_Y;
  const height = Math.max(...sizes.map((_, c) => colH(c)));
  const width = ncols * NODE_W + (ncols - 1) * GAP_X;
  const nodes = members.map((job): DagNode => {
    const col = cols.get(job.key) as number;
    return { job, col, left: col * COL_STEP, top: (height - colH(col)) / 2 + (rows.get(job.key) as number) * ROW_STEP };
  });
  const at = new Map(nodes.map((n) => [n.job.key, n]));
  const edges: DagEdge[] = [];
  for (const t of nodes) {
    for (const need of t.job.needs) {
      const s = at.get(need);
      if (s === undefined) continue;
      const sx = s.left + NODE_W;
      const ys = s.top + NODE_H / 2;
      const yt = t.top + NODE_H / 2;
      const skip = t.col > s.col + 1;
      let d: string;
      if (ys === yt) {
        d = `M${sx + 1} ${ys}H${sx + 25}M${sx + 21} ${ys - 3.5}L${sx + 25} ${ys} ${sx + 21} ${ys + 3.5}`;
      } else {
        d = `M${sx + 1} ${ys}C${sx + 14} ${ys} ${sx + 12} ${yt} ${sx + 25} ${yt}`;
      }
      if (skip) d += `H${t.left - 3}`;
      const done = parseJobStatus(s.job.status);
      edges.push({ id: `${s.job.key}>${t.job.key}`, d, state: done.kind === "known" && done.value === "merged" ? "done" : "pending" });
    }
  }
  return { width, height, nodes, edges };
}
