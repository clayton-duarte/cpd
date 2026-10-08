import { describe, expect, it } from 'vitest';
import { buildGraph } from '../layout/toGraph';
import { planToCpdData, type DaemonJob, type DaemonJobStatus } from './fromPlan';

const conversation = { id: 42, title: 'Test Conversation' };
const JOB_CARD_SIZE = { width: 220, height: 96 };

describe('planToCpdData', () => {
  it('produces a well-formed CpdData with one workflow and no jobs for an empty list', () => {
    const result = planToCpdData([], conversation);

    expect(result.projects).toHaveLength(1);
    expect(result.sessions).toHaveLength(1);
    expect(result.workflows).toHaveLength(1);
    expect(result.jobs).toHaveLength(0);
    expect(result.workflows[0].id).toBe(String(conversation.id));
    expect(result.workflows[0].title).toBe(conversation.title);
    expect(result.workflows[0].jobIds).toEqual([]);
  });

  it('keeps jobs and needs edges with stable ids', () => {
    const jobs: DaemonJob[] = [
      { id: 'a', title: 'A', status: 'done', needs: [] },
      { id: 'b', title: 'B', status: 'queued', needs: ['a'] },
    ];

    const result = planToCpdData(jobs, conversation);

    expect(result.jobs.map((j) => j.id)).toEqual(['a', 'b']);
    const jobB = result.jobs.find((j) => j.id === 'b');
    expect(jobB?.needs).toEqual(['a']);
    expect(result.workflows[0].jobIds).toEqual(['a', 'b']);
  });

  it('passes through status unchanged for all five daemon statuses', () => {
    const statuses: DaemonJobStatus[] = ['draft', 'queued', 'running', 'done', 'failed'];
    const jobs: DaemonJob[] = statuses.map((status, i) => ({
      id: `j${i}`,
      title: `Job ${i}`,
      status,
      needs: [],
    }));

    const result = planToCpdData(jobs, conversation);

    result.jobs.forEach((job, i) => {
      expect(job.status).toBe(statuses[i]);
    });
  });

  it('drops a needs entry referencing an unknown id but keeps the job', () => {
    const jobs: DaemonJob[] = [
      { id: 'a', title: 'A', status: 'draft', needs: ['ghost'] },
    ];

    const result = planToCpdData(jobs, conversation);

    expect(result.jobs).toHaveLength(1);
    expect(result.jobs[0].needs).toEqual([]);
  });

  it('feeds buildGraph without throwing and produces expected node/edge counts', () => {
    const jobs: DaemonJob[] = [
      { id: 'a', title: 'A', status: 'done', needs: [] },
      { id: 'b', title: 'B', status: 'running', needs: ['a'] },
      { id: 'c', title: 'C', status: 'queued', needs: ['b'] },
    ];

    const data = planToCpdData(jobs, conversation);

    let graph;
    expect(() => {
      graph = buildGraph(data, JOB_CARD_SIZE);
    }).not.toThrow();

    // 1 workflow node + 3 job nodes
    expect(graph!.nodes).toHaveLength(4);
    // 2 edges: a->b, b->c
    expect(graph!.edges).toHaveLength(2);
  });
});
