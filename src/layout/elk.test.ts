import { describe, it, expect } from 'vitest';
import { elkLayout } from './elk';
import { buildGraph } from './toGraph';
import { computeWaves } from './waves';
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

  // D72: append-stability is asserted as WAVE MEMBERSHIP ONLY.
  // Pixel drift is deliberately NOT asserted. Appending to a wave redistributes that
  // wave's own members by design (a 3-card group becoming 4 moves cards by a full row),
  // and cross-wave crossing-minimisation may reorder siblings. Both are the layout
  // engine doing its job. Whether the canvas stays readable when work is appended is a
  // VISUAL property, validated by hand in the Phase A review (D69) -- not by a px bound.
  it('append-stability (D72: wave membership only)', async () => {
    const graph = simpleGraph();
    await elkLayout(graph);

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

    const beforeWaves = computeWaves(sampleData.jobs);
    const afterWaves = computeWaves(appendedData.jobs);

    // Wave membership is stable: appending a job does not change any
    // pre-existing job's wave index.
    for (const job of sampleData.jobs) {
      expect(after[job.id]).toBeDefined();
      expect(afterWaves.get(job.id)).toBe(beforeWaves.get(job.id));
    }
  });
});

describe('buildGraph', () => {
  it('produces the expected parent/child nesting for the fixture', () => {
    const graph = buildGraph(sampleData, JOB_CARD_SIZE);
    const byId = new Map(graph.nodes.map((n) => [n.id, n]));

    const workflowNode = byId.get('workflow:w1');
    expect(workflowNode).toBeDefined();
    expect(workflowNode?.parentId).toBeUndefined();

    const j1 = byId.get('j1');
    expect(j1?.parentId).toBe('workflow:w1');
    expect(j1?.width).toBe(220);
    expect(j1?.height).toBe(80);
  });

  it('creates one edge per needs relationship', () => {
    const graph = buildGraph(sampleData, JOB_CARD_SIZE);
    const totalNeeds = sampleData.jobs.reduce((sum, j) => sum + j.needs.length, 0);
    expect(graph.edges.length).toBe(totalNeeds);
  });
});
