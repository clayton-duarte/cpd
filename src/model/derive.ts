/**
 * Pure derivation functions: data -> visual signals.
 * No React, no Mantine, no DOM.
 */
import type { ColorToken, FlowLabel, Job, JobStatus, Phase } from './types';

export type { ColorToken };

export function jobColor(job: Job): ColorToken {
  if (job.status === 'done' || job.status === 'awaiting_confirm') return 'green';
  if (job.owner === 'you') return 'red';
  if (job.owner === 'agents') return 'blue';
  return 'white';
}

const STATUS_ICONS: Record<JobStatus, string> = {
  draft: 'circle-dashed',
  queued: 'circle-dashed',
  running: 'loader-2',
  blocked: 'hand-stop',
  failed: 'x',
  waiting: 'clock',
  paused: 'player-pause',
  awaiting_confirm: 'checks',
  done: 'check',
  skipped: 'ban',
};

export function statusIcon(status: JobStatus): string {
  return STATUS_ICONS[status];
}

export function isDashed(job: Job): boolean {
  return job.status === 'draft';
}

export function isDimmed(job: Job): boolean {
  return job.status === 'skipped';
}

export function tierLabel(job: Job): string {
  return job.tier ?? '··';
}

export function showAttempt(job: Job): boolean {
  return job.attempt >= 2;
}

const PHASE_TO_FLOW: Record<Phase, FlowLabel> = {
  upkeep: 'Context',
  draw: 'Context',
  main1: 'Plan',
  combat: 'Dispatch',
  main2: 'Review',
  end: 'Ship',
};

export function flowLabel(phase: Phase): FlowLabel {
  return PHASE_TO_FLOW[phase];
}

export const FLOW_ORDER: FlowLabel[] = ['Context', 'Plan', 'Dispatch', 'Review', 'Ship'];
