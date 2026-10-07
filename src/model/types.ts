/**
 * PROVISIONAL. These types describe only what the canvas renders today.
 * They are not a contract. Expect churn. Do not build anything on their stability.
 */

/** Who must act next. Drives card border color. */
export type Owner = 'you' | 'agents' | 'neutral'; // red | blue | white

/** Model tier letter + effort digit, e.g. 'O3', 'S1', 'H0', 'L0'. */
export type TierCode = `${'O' | 'S' | 'H' | 'L'}${0 | 1 | 2 | 3 | 4}`;

/** Engine-internal lifecycle. NEVER shown in the UI — see flow labels. */
export type Phase = 'upkeep' | 'draw' | 'main1' | 'combat' | 'main2' | 'end';

/** What the user reads in the workflow header. */
export type FlowLabel = 'Context' | 'Plan' | 'Dispatch' | 'Review' | 'Ship';

export type JobStatus =
  | 'draft' | 'queued' | 'running' | 'blocked' | 'failed'
  | 'waiting' | 'paused' | 'awaiting_confirm' | 'done' | 'skipped';

export type JobProfile = 'lead' | 'builder' | 'reviewer' | 'scribe' | 'aux';

export interface Job {
  id: string;
  workflowId: string;
  title: string;
  status: JobStatus;
  owner: Owner;
  profile: JobProfile;
  /** Undefined on a draft Opus has not yet tiered — renders as a muted placeholder. */
  tier?: TierCode;
  attempt: number;          // 1-based; badge shown from 2 on
  needs: string[];          // ids of jobs this one depends on
  artifactCount: number;
  /** A steering note is pending pickup. A MARKER, not a status: the job still runs. */
  steeringPending: boolean;
}

export interface Workflow {
  id: string;
  sessionId: string;
  title: string;
  pr?: number;
  phase: Phase;
  attempt: number;
  owner: Owner;
  jobIds: string[];
}

export interface Session {
  id: string;
  projectId: string;
  name: string;
  leadTier: TierCode;
  workflowIds: string[];
}

export interface Project {
  id: string;
  name: string;
  repos: string[];
  sessionIds: string[];
}

export interface CpdData {
  projects: Project[];
  sessions: Session[];
  workflows: Workflow[];
  jobs: Job[];
}

export const isCommitted = (j: Job) => j.status !== 'draft';
