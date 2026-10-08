import { describe, it, expect } from 'vitest';
import { sampleData } from './sample';
import type { Job, JobStatus } from '../model/types';

function jobsById(): Map<string, Job> {
  return new Map(sampleData.jobs.map((j) => [j.id, j]));
}

const ALL_STATUSES: JobStatus[] = [
  'draft', 'queued', 'running', 'blocked', 'failed',
  'waiting', 'paused', 'awaiting_confirm', 'done', 'skipped',
];

function jobsByWorkflow(workflowId: string): Job[] {
  return sampleData.jobs.filter((j) => j.workflowId === workflowId);
}

function hasCycle(jobs: Job[]): boolean {
  const byId = new Map(jobs.map((j) => [j.id, j]));
  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Map<string, number>(jobs.map((j) => [j.id, WHITE]));

  function visit(id: string): boolean {
    color.set(id, GRAY);
    const job = byId.get(id);
    if (job) {
      for (const dep of job.needs) {
        const depColor = color.get(dep);
        if (depColor === GRAY) return true;
        if (depColor === WHITE && visit(dep)) return true;
      }
    }
    color.set(id, BLACK);
    return false;
  }

  for (const j of jobs) {
    if (color.get(j.id) === WHITE) {
      if (visit(j.id)) return true;
    }
  }
  return false;
}

describe('sampleData fixture', () => {
  it('every job.workflowId refers to an existing workflow', () => {
    const workflowIds = new Set(sampleData.workflows.map((w) => w.id));
    for (const job of sampleData.jobs) {
      expect(workflowIds.has(job.workflowId)).toBe(true);
    }
  });

  it('every id in workflow.jobIds exists as a job', () => {
    const jobIds = new Set(sampleData.jobs.map((j) => j.id));
    for (const workflow of sampleData.workflows) {
      for (const id of workflow.jobIds) {
        expect(jobIds.has(id)).toBe(true);
      }
    }
  });

  it('every id in job.needs exists in the same workflow', () => {
    for (const job of sampleData.jobs) {
      const siblings = new Set(jobsByWorkflow(job.workflowId).map((j) => j.id));
      for (const dep of job.needs) {
        expect(siblings.has(dep)).toBe(true);
      }
    }
  });

  it('the needs graph is acyclic', () => {
    expect(hasCycle(sampleData.jobs)).toBe(false);
  });

  it('the needs graph DFS correctly detects an injected cycle', () => {
    const cyclicJobs: Job[] = [
      { ...sampleData.jobs[0], id: 'x1', needs: ['x2'] },
      { ...sampleData.jobs[0], id: 'x2', needs: ['x1'] },
    ];
    expect(hasCycle(cyclicJobs)).toBe(true);
  });

  it('every JobStatus value appears at least once', () => {
    const statuses = new Set(sampleData.jobs.map((j) => j.status));
    for (const status of ALL_STATUSES) {
      expect(statuses.has(status)).toBe(true);
    }
  });

  it('at least one job has steeringPending true', () => {
    expect(sampleData.jobs.some((j) => j.steeringPending)).toBe(true);
  });

  it('at least one job has no tier', () => {
    expect(sampleData.jobs.some((j) => j.tier === undefined)).toBe(true);
  });

  it('all job ids are unique', () => {
    const ids = sampleData.jobs.map((j) => j.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('all ids are unique across the whole fixture', () => {
    const ids = [
      ...sampleData.projects.map((p) => p.id),
      ...sampleData.sessions.map((s) => s.id),
      ...sampleData.workflows.map((w) => w.id),
      ...sampleData.jobs.map((j) => j.id),
    ];
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('project.sessionIds and session membership agree both ways', () => {
    const sessionIds = new Set(sampleData.sessions.map((s) => s.id));
    for (const project of sampleData.projects) {
      for (const id of project.sessionIds) {
        expect(sessionIds.has(id)).toBe(true);
      }
    }
    const referenced = new Set(sampleData.projects.flatMap((p) => p.sessionIds));
    for (const session of sampleData.sessions) {
      expect(referenced.has(session.id)).toBe(true);
    }
  });

  it('session.workflowIds and workflow membership agree both ways', () => {
    const workflowIds = new Set(sampleData.workflows.map((w) => w.id));
    for (const session of sampleData.sessions) {
      for (const id of session.workflowIds) {
        expect(workflowIds.has(id)).toBe(true);
      }
    }
    const referenced = new Set(sampleData.sessions.flatMap((s) => s.workflowIds));
    for (const workflow of sampleData.workflows) {
      expect(referenced.has(workflow.id)).toBe(true);
    }
  });

  it('workflow.jobIds and job membership agree both ways', () => {
    const jobIds = new Set(sampleData.jobs.map((j) => j.id));
    for (const workflow of sampleData.workflows) {
      for (const id of workflow.jobIds) {
        expect(jobIds.has(id)).toBe(true);
      }
    }
    const referenced = new Set(sampleData.workflows.flatMap((w) => w.jobIds));
    for (const job of sampleData.jobs) {
      expect(referenced.has(job.id)).toBe(true);
    }
  });

  it('every session has a leadTier', () => {
    for (const session of sampleData.sessions) {
      expect(session.leadTier).toBeTruthy();
    }
  });

  it('jobsById resolves every id present in the fixture', () => {
    const byId = jobsById();
    expect(byId.size).toBe(sampleData.jobs.length);
  });

  it('no string field matches the public-repo tripwire pattern', () => {
    const tripwire = /missionlane|jira|slack|EEC-/i;
    const seen: string[] = [];
    const walk = (val: unknown) => {
      if (typeof val === 'string') {
        seen.push(val);
      } else if (Array.isArray(val)) {
        val.forEach(walk);
      } else if (val && typeof val === 'object') {
        Object.values(val).forEach(walk);
      }
    };
    walk(sampleData);
    for (const s of seen) {
      expect(tripwire.test(s)).toBe(false);
    }
  });
});
