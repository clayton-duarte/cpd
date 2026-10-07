import type { CpdData, Job } from '../model/types';
import type { LayoutGraph } from './types';
import { groupByWave } from './waves';

/**
 * Build the LayoutGraph from CPD data: session frame > workflow > wave groups > job nodes.
 * Nesting mirrors the visual hierarchy: each session is a frame containing its
 * workflows; each workflow contains one wave-group node per wave of its jobs;
 * each wave group contains the job nodes for that wave.
 */
export function buildGraph(data: CpdData, jobCardSize: { width: number; height: number }): LayoutGraph {
  const nodes: LayoutGraph['nodes'] = [];
  const edges: LayoutGraph['edges'] = [];

  // Reserved space inside each compound node for the chrome rendered on top
  // of it (frame label + lead card, or workflow header strip). ELK treats
  // these as ordinary padding, not children, so job layout never collides
  // with the overlay drawn by the Canvas.
  const SESSION_TOP_PADDING = 110; // label row + LeadCard height
  const WORKFLOW_TOP_PADDING = 56; // WorkflowHeader row
  const LEAD_CARD_SIZE = { width: 220, height: 64 };

  for (const session of data.sessions) {
    const sessionId = `session:${session.id}`;
    nodes.push({
      id: sessionId,
      width: 0,
      height: 0,
      layoutOptions: {
        'elk.padding': `[top=${SESSION_TOP_PADDING},left=20,bottom=20,right=20]`,
      },
    });

    // The LeadCard sits at the frame's top-left, before the first wave.
    // It is laid out as a real (non-job) child positioned first so model
    // order keeps it ahead of the workflow in the visual stack.
    nodes.push({
      id: `lead:${session.id}`,
      width: LEAD_CARD_SIZE.width,
      height: LEAD_CARD_SIZE.height,
      parentId: sessionId,
    });

    for (const workflowId of session.workflowIds) {
      const workflow = data.workflows.find((w) => w.id === workflowId);
      if (!workflow) continue;

      const workflowNodeId = `workflow:${workflow.id}`;
      nodes.push({
        id: workflowNodeId,
        width: 0,
        height: 0,
        parentId: sessionId,
        layoutOptions: {
          'elk.padding': `[top=${WORKFLOW_TOP_PADDING},left=0,bottom=0,right=0]`,
        },
      });

      const workflowJobs: Job[] = workflow.jobIds
        .map((jobId) => data.jobs.find((j) => j.id === jobId))
        .filter((j): j is Job => j !== undefined);

      const waveGroups = groupByWave(workflowJobs);

      waveGroups.forEach((jobsInWave, waveIndex) => {
        const waveNodeId = `wave:${workflow.id}:${waveIndex}`;
        nodes.push({ id: waveNodeId, width: 0, height: 0, parentId: workflowNodeId });

        for (const job of [...jobsInWave].reverse()) {
          nodes.push({
            id: job.id,
            width: jobCardSize.width,
            height: jobCardSize.height,
            parentId: waveNodeId,
          });
        }
      });

      for (const job of workflowJobs) {
        for (const dep of job.needs) {
          edges.push({ id: `${dep}->${job.id}`, source: dep, target: job.id });
        }
      }
    }
  }

  return { nodes, edges };
}
