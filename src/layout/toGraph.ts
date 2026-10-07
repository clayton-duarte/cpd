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

  for (const session of data.sessions) {
    const sessionId = `session:${session.id}`;
    nodes.push({ id: sessionId, width: 0, height: 0 });

    for (const workflowId of session.workflowIds) {
      const workflow = data.workflows.find((w) => w.id === workflowId);
      if (!workflow) continue;

      const workflowNodeId = `workflow:${workflow.id}`;
      nodes.push({ id: workflowNodeId, width: 0, height: 0, parentId: sessionId });

      const workflowJobs: Job[] = workflow.jobIds
        .map((jobId) => data.jobs.find((j) => j.id === jobId))
        .filter((j): j is Job => j !== undefined);

      const waveGroups = groupByWave(workflowJobs);

      waveGroups.forEach((jobsInWave, waveIndex) => {
        const waveNodeId = `wave:${workflow.id}:${waveIndex}`;
        nodes.push({ id: waveNodeId, width: 0, height: 0, parentId: workflowNodeId });

        for (const job of jobsInWave) {
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
