import type { CpdData, Job } from '../model/types';
import type { LayoutGraph } from './types';
import { groupByWave } from './waves';

/**
 * Build the LayoutGraph from CPD data: workflow > job nodes. Workflows are
 * top-level nodes; jobs are direct children of their workflow. Wave index
 * still determines left-to-right ordering (via an ELK layer-id hint), but
 * is not drawn as a container -- longest-path layering is a layout
 * coordinate, not a visual barrier (D74).
 */
export function buildGraph(data: CpdData, jobCardSize: { width: number; height: number }): LayoutGraph {
  const nodes: LayoutGraph['nodes'] = [];
  const edges: LayoutGraph['edges'] = [];

  // Reserved space inside each workflow node for the WorkflowHeader strip
  // rendered on top of it. ELK treats this as ordinary padding, not a
  // child, so job layout never collides with the overlay drawn by Canvas.
  // Value = --space-2 (above) + header line height (~24px) + --space-2
  // (below) = 40, tightened from 56 once B1 removed the breadcrumb row.
  const WORKFLOW_TOP_PADDING = 40;

  for (const session of data.sessions) {
    for (const workflowId of session.workflowIds) {
      const workflow = data.workflows.find((w) => w.id === workflowId);
      if (!workflow) continue;

      const workflowNodeId = `workflow:${workflow.id}`;
      nodes.push({
        id: workflowNodeId,
        width: 0,
        height: 0,
        layoutOptions: {
          'elk.padding': `[top=${WORKFLOW_TOP_PADDING},left=20,bottom=20,right=20]`,
        },
      });

      const workflowJobs: Job[] = workflow.jobIds
        .map((jobId) => data.jobs.find((j) => j.id === jobId))
        .filter((j): j is Job => j !== undefined);

      const waveGroups = groupByWave(workflowJobs);

      // Namespace node ids per workflow so same-named job ids in different
      // workflows never collide or connect: workflows are independent
      // graphs with zero edges crossing between them.
      const nodeId = (jobId: string) => `${workflow.id}:${jobId}`;

      waveGroups.forEach((jobsInWave, waveIndex) => {
        for (const job of [...jobsInWave].reverse()) {
          nodes.push({
            id: nodeId(job.id),
            width: jobCardSize.width,
            height: jobCardSize.height,
            parentId: workflowNodeId,
            jobId: job.id,
            // Ordering key only, not a container: keeps jobs reading
            // left-to-right in dependency order without drawing a box
            // around the wave (D74).
            layoutOptions: { 'elk.layered.layering.layerId': String(waveIndex) },
          });
        }
      });

      for (const job of workflowJobs) {
        for (const dep of job.needs) {
          edges.push({ id: `${nodeId(dep)}->${nodeId(job.id)}`, source: nodeId(dep), target: nodeId(job.id) });
        }
      }
    }
  }

  return { nodes, edges };
}
