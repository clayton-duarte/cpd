import type { CpdData, Job, JobStatus } from './types';

/**
 * Minimal daemon-side job shape (H4). A strict subset of the UI's Job:
 * the five statuses below all exist in the UI's ten-value JobStatus.
 */
export type DaemonJobStatus = 'draft' | 'queued' | 'running' | 'done' | 'failed';

export interface DaemonJob {
  id: string;
  title: string;
  status: DaemonJobStatus;
  needs: string[];
  command?: string;
}

/**
 * Pure adapter: turns a daemon plan (jobs + conversation) into the CpdData
 * shape the canvas already draws via buildGraph. No fetch, no React.
 */
export function planToCpdData(
  jobs: readonly DaemonJob[],
  conversation: { id: number; title: string },
): CpdData {
  const workflowId = String(conversation.id);
  const sessionId = 's1';
  const projectId = 'p1';

  const knownIds = new Set(jobs.map((j) => j.id));

  const uiJobs: Job[] = jobs.map((job) => ({
    id: job.id,
    workflowId,
    title: job.title,
    status: job.status as JobStatus,
    owner: 'neutral',
    profile: 'builder',
    tier: undefined,
    attempt: 1,
    needs: job.needs.filter((dep) => knownIds.has(dep)),
    artifactCount: 0,
    steeringPending: false,
  }));

  return {
    projects: [
      {
        id: projectId,
        name: conversation.title,
        repos: [],
        sessionIds: [sessionId],
      },
    ],
    sessions: [
      {
        id: sessionId,
        projectId,
        name: conversation.title,
        leadTier: 'H0',
        workflowIds: [workflowId],
      },
    ],
    workflows: [
      {
        id: workflowId,
        sessionId,
        title: conversation.title,
        phase: 'draw',
        attempt: 1,
        owner: 'neutral',
        jobIds: jobs.map((j) => j.id),
      },
    ],
    jobs: uiJobs,
  };
}
