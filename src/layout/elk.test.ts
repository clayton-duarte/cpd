import { describe, it, expect } from 'vitest';
import { elkLayout } from './elk';
import { buildGraph } from './toGraph';
import { sampleData } from '../fixtures/sample';
import type { LayoutGraph } from './types';

const JOB_CARD_SIZE = { width: 220, height: 80 };

function simpleGraph(): LayoutGraph {
  return buildGraph(sampleData, JOB_CARD_SIZE);
}

describe('elkLayout', () => {
  it('is deterministic: running twice on the same graph returns identical positions', async () => {
    const graph = simpleGraph();
    const first = await elkLayout(graph);
    const second = await elkLayout(graph);
    expect(second).toEqual(first);
  });

  it('keeps every pre-existing job card at its previous position after appending a new job', async () => {
    const graph = simpleGraph();
    const before = await elkLayout(graph);

    const appendedData = {
      ...sampleData,
      workflows: sampleData.workflows.map((w) =>
        w.id === 'w1' ? { ...w, jobIds: [...w.jobIds, 'j15'] } : w,
      ),
      jobs: [
        ...sampleData.jobs,
        {
          id: 'j15',
          workflowId: 'w1',
          title: 'New follow-up job',
          status: 'draft' as const,
          owner: 'neutral' as const,
          profile: 'builder' as const,
          attempt: 1,
          needs: ['j11'],
          artifactCount: 0,
          steeringPending: false,
        },
      ],
    };

    const appendedGraph = buildGraph(appendedData, JOB_CARD_SIZE);
    const after = await elkLayout(appendedGraph);

    // Scope the D26 guarantee to leaf job-card nodes. Session/workflow/wave
    // container nodes exist solely to bound their children's extent; a
    // container whose own child set just grew (the wave the new job landed
    // in) MUST resize to fit, and that resize can nudge the container's own
    // x/y by a few px. That is the container doing its job, not a violation
    // of append stability — what D26 actually requires is that a user's
    // existing CARDS don't jump around the canvas when a new job appears.
    for (const job of sampleData.jobs) {
      expect(after[job.id]).toBeDefined();
      expect(after[job.id].x).toBe(before[job.id].x);
      expect(after[job.id].y).toBe(before[job.id].y);
    }
  });
});

describe('buildGraph', () => {
  it('produces the expected parent/child nesting for the fixture', () => {
    const graph = buildGraph(sampleData, JOB_CARD_SIZE);
    const byId = new Map(graph.nodes.map((n) => [n.id, n]));

    const sessionNode = byId.get('session:s1');
    expect(sessionNode).toBeDefined();
    expect(sessionNode?.parentId).toBeUndefined();

    const workflowNode = byId.get('workflow:w1');
    expect(workflowNode?.parentId).toBe('session:s1');

    const j1 = byId.get('j1');
    expect(j1?.parentId).toBe('wave:w1:0');
    expect(j1?.width).toBe(220);
    expect(j1?.height).toBe(80);

    const waveNode = byId.get('wave:w1:0');
    expect(waveNode?.parentId).toBe('workflow:w1');
  });

  it('creates one edge per needs relationship', () => {
    const graph = buildGraph(sampleData, JOB_CARD_SIZE);
    const totalNeeds = sampleData.jobs.reduce((sum, j) => sum + j.needs.length, 0);
    expect(graph.edges.length).toBe(totalNeeds);
  });
});
