import { describe, expect, it } from 'vitest';
import type { Job, JobStatus, Phase } from './types';
import {
  FLOW_ORDER,
  flowLabel,
  isDashed,
  isDimmed,
  jobColor,
  showAttempt,
  statusIcon,
  tierLabel,
} from './derive';

const baseJob: Job = {
  id: 'jx',
  workflowId: 'w1',
  title: 'Base job',
  status: 'queued',
  owner: 'neutral',
  profile: 'builder',
  attempt: 1,
  needs: [],
  artifactCount: 0,
  steeringPending: false,
};

const job = (overrides: Partial<Job>): Job => ({ ...baseJob, ...overrides });

describe('jobColor', () => {
  it('is green when status is done, regardless of owner', () => {
    expect(jobColor(job({ status: 'done', owner: 'you' }))).toBe('green');
    expect(jobColor(job({ status: 'done', owner: 'agents' }))).toBe('green');
  });

  it('is green when status is awaiting_confirm', () => {
    expect(jobColor(job({ status: 'awaiting_confirm', owner: 'neutral' }))).toBe('green');
  });

  it('a blocked job with owner "you" is red (j5)', () => {
    expect(jobColor(job({ status: 'blocked', owner: 'you' }))).toBe('red');
  });

  it('a failed job with owner "agents" is BLUE, not red (j8)', () => {
    expect(jobColor(job({ status: 'failed', owner: 'agents' }))).toBe('blue');
  });

  it('is white otherwise', () => {
    expect(jobColor(job({ status: 'queued', owner: 'neutral' }))).toBe('white');
  });
});

describe('statusIcon', () => {
  it('maps known statuses to their icons', () => {
    expect(statusIcon('draft')).toBe('circle-dashed');
    expect(statusIcon('queued')).toBe('circle-dashed');
    expect(statusIcon('running')).toBe('loader-2');
    expect(statusIcon('blocked')).toBe('hand-stop');
    expect(statusIcon('failed')).toBe('x');
    expect(statusIcon('waiting')).toBe('clock');
    expect(statusIcon('paused')).toBe('player-pause');
    expect(statusIcon('awaiting_confirm')).toBe('checks');
    expect(statusIcon('done')).toBe('check');
    expect(statusIcon('skipped')).toBe('ban');
  });

  it('is exhaustive: every JobStatus returns a non-empty string', () => {
    const lookup: Record<JobStatus, string> = {
      draft: statusIcon('draft'),
      queued: statusIcon('queued'),
      running: statusIcon('running'),
      blocked: statusIcon('blocked'),
      failed: statusIcon('failed'),
      waiting: statusIcon('waiting'),
      paused: statusIcon('paused'),
      awaiting_confirm: statusIcon('awaiting_confirm'),
      done: statusIcon('done'),
      skipped: statusIcon('skipped'),
    };
    for (const status of Object.keys(lookup) as JobStatus[]) {
      expect(lookup[status].length).toBeGreaterThan(0);
    }
  });

  it('the eight non-shared icons are all distinct', () => {
    const distinctStatuses: JobStatus[] = [
      'running', 'blocked', 'failed', 'waiting',
      'paused', 'awaiting_confirm', 'done', 'skipped',
    ];
    const icons = distinctStatuses.map(statusIcon);
    expect(new Set(icons).size).toBe(icons.length);
  });
});

describe('isDashed', () => {
  it('is true only for status draft', () => {
    expect(isDashed(job({ status: 'draft' }))).toBe(true);
    expect(isDashed(job({ status: 'queued' }))).toBe(false);
    expect(isDashed(job({ status: 'running' }))).toBe(false);
  });
});

describe('isDimmed', () => {
  it('is true only for status skipped', () => {
    expect(isDimmed(job({ status: 'skipped' }))).toBe(true);
    expect(isDimmed(job({ status: 'done' }))).toBe(false);
  });
});

describe('tierLabel', () => {
  it('returns the tier when present', () => {
    expect(tierLabel(job({ tier: 'S1' }))).toBe('S1');
  });

  it('returns the placeholder when absent', () => {
    expect(tierLabel(job({ tier: undefined }))).toBe('··');
  });
});

describe('showAttempt', () => {
  it('is false for attempt 1', () => {
    expect(showAttempt(job({ attempt: 1 }))).toBe(false);
  });

  it('is true for attempt >= 2', () => {
    expect(showAttempt(job({ attempt: 2 }))).toBe(true);
    expect(showAttempt(job({ attempt: 3 }))).toBe(true);
  });
});

describe('flowLabel', () => {
  const mapping: [Phase, string][] = [
    ['upkeep', 'Context'],
    ['draw', 'Context'],
    ['main1', 'Plan'],
    ['combat', 'Dispatch'],
    ['main2', 'Review'],
    ['end', 'Ship'],
  ];

  it('never shows the engine term', () => {
    for (const [phase, label] of mapping) {
      expect(flowLabel(phase)).toBe(label);
    }
  });

  it('FLOW_ORDER lists the five labels in order', () => {
    expect(FLOW_ORDER).toEqual(['Context', 'Plan', 'Dispatch', 'Review', 'Ship']);
  });
});
