import { describe, expect, it } from 'vitest';
import { buildGraph } from './toGraph';
import { sampleData } from '../fixtures/sample';

const JOB_CARD_SIZE = { width: 220, height: 96 };

describe('buildGraph', () => {
  it('produces two disjoint workflow subgraphs with zero edges crossing between them', () => {
    const graph = buildGraph(sampleData, JOB_CARD_SIZE);

    const workflowIds = sampleData.workflows.map((wf) => wf.id);
    expect(workflowIds.length).toBeGreaterThanOrEqual(2);

    // Every job node id is namespaced under its own workflow.
    const jobNodes = graph.nodes.filter((n) => n.jobId !== undefined);
    expect(jobNodes.length).toBeGreaterThan(0);
    for (const node of jobNodes) {
      expect(node.id).toBe(`${node.parentId?.replace('workflow:', '')}:${node.jobId}`);
    }

    // No edge connects node ids namespaced under different workflows.
    const workflowOf = (nodeId: string) => nodeId.split(':')[0];
    for (const edge of graph.edges) {
      expect(workflowOf(edge.source)).toBe(workflowOf(edge.target));
    }

    // Each workflow's job nodes form their own group, no overlap in ids.
    const idsByWorkflow = new Map<string, Set<string>>();
    for (const node of jobNodes) {
      const wfId = workflowOf(node.id);
      const set = idsByWorkflow.get(wfId) ?? new Set<string>();
      set.add(node.id);
      idsByWorkflow.set(wfId, set);
    }
    const allIds = [...idsByWorkflow.values()].flatMap((set) => [...set]);
    expect(new Set(allIds).size).toBe(allIds.length);
  });
});
