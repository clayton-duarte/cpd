import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, waitFor, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import type { Node } from '@xyflow/react';
import type { LayoutGraph, LayoutResult } from '../layout/types';
import type { CpdData, Job, JobStatus, Owner } from '../model/types';

/**
 * elkLayout is mocked with a deterministic, synchronous-ish fake so the falsification table
 * below is a pure call-count assertion on OUR code (when do we ask for layout), not a test of
 * elkjs/ELK itself (already covered by src/layout/elk.test.ts).
 */
const elkLayoutMock = vi.fn(async (graph: LayoutGraph): Promise<LayoutResult> => {
  const result: LayoutResult = {};
  graph.nodes.forEach((n, i) => {
    result[n.id] = { id: n.id, x: i * 10, y: 0, width: n.width, height: n.height };
  });
  return result;
});

vi.mock('../layout/elk', () => ({
  elkLayout: (graph: LayoutGraph) => elkLayoutMock(graph),
}));

/** Records the `nodes` array identity React Flow is given on every render, so test B can assert
 * per-node object identity is preserved when a node's own data hasn't changed. */
const capturedNodes: Node[][] = [];

vi.mock('@xyflow/react', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@xyflow/react')>();
  function RecordingReactFlow(props: Parameters<typeof actual.ReactFlow>[0]) {
    capturedNodes.push(props.nodes ?? []);
    return <actual.ReactFlow {...props} />;
  }
  return { ...actual, ReactFlow: RecordingReactFlow };
});

const { Canvas } = await import('./Canvas');
const { ReactFlowProvider } = await import('@xyflow/react');

function job(id: string, title: string, overrides: Partial<Job> = {}): Job {
  return {
    id,
    workflowId: 'w1',
    title,
    status: 'running' as JobStatus,
    owner: 'agents' as Owner,
    profile: 'builder',
    attempt: 1,
    needs: [],
    artifactCount: 0,
    steeringPending: false,
    ...overrides,
  };
}

function makeData(jobTitle: string, opts: { extraJob?: boolean; status?: JobStatus } = {}): CpdData {
  const jobs = [job('j1', jobTitle, { status: opts.status ?? 'running' })];
  if (opts.extraJob) jobs.push(job('j2', 'second', { status: 'queued' }));

  return {
    projects: [{ id: 'p1', name: 'P', repos: [], sessionIds: ['s1'] }],
    sessions: [{ id: 's1', projectId: 'p1', name: 'S', leadTier: 'H0', workflowIds: ['w1'] }],
    workflows: [
      {
        id: 'w1',
        sessionId: 's1',
        title: 'WF',
        phase: 'draw',
        attempt: 1,
        owner: 'neutral',
        jobIds: opts.extraJob ? ['j1', 'j2'] : ['j1'],
      },
    ],
    jobs,
  };
}

function renderCanvas(data: CpdData, selectedJobId?: string | null) {
  return render(
    <MantineProvider>
      <ReactFlowProvider>
        <Canvas data={data} selectedJobId={selectedJobId} />
      </ReactFlowProvider>
    </MantineProvider>,
  );
}

beforeEach(() => {
  elkLayoutMock.mockClear();
  capturedNodes.length = 0;
});

describe('Canvas falsification table (L6)', () => {
  it('A: layout runs once on mount, never again from an unrelated rerender with unchanged data', async () => {
    const data = makeData('Alpha');
    const { rerender } = renderCanvas(data);
    await waitFor(() => expect(elkLayoutMock).toHaveBeenCalledTimes(1));

    // Same content, brand-new object identities -- exactly what a fresh `planToCpdData(...)`
    // call on every App render would hand down.
    rerender(
      <MantineProvider>
        <ReactFlowProvider>
          <Canvas data={makeData('Alpha')} />
        </ReactFlowProvider>
      </MantineProvider>,
    );
    await new Promise((r) => setTimeout(r, 0));
    expect(elkLayoutMock).toHaveBeenCalledTimes(1);
  });

  it('C: three "messages" SSE frames (data-only, unchanged topology) leave the layout call count unchanged', async () => {
    const data = makeData('Alpha');
    const { rerender } = renderCanvas(data);
    await waitFor(() => expect(elkLayoutMock).toHaveBeenCalledTimes(1));

    for (let i = 0; i < 3; i += 1) {
      rerender(
        <MantineProvider>
          <ReactFlowProvider>
            <Canvas data={makeData('Alpha')} />
          </ReactFlowProvider>
        </MantineProvider>,
      );
    }
    await new Promise((r) => setTimeout(r, 0));
    expect(elkLayoutMock).toHaveBeenCalledTimes(1);
  });

  it('adding one node (topology change) triggers exactly one additional layout call', async () => {
    const data = makeData('Alpha');
    const { rerender } = renderCanvas(data);
    await waitFor(() => expect(elkLayoutMock).toHaveBeenCalledTimes(1));

    rerender(
      <MantineProvider>
        <ReactFlowProvider>
          <Canvas data={makeData('Alpha', { extraJob: true })} />
        </ReactFlowProvider>
      </MantineProvider>,
    );

    await waitFor(() => expect(elkLayoutMock).toHaveBeenCalledTimes(2));
  });

  it('B: a node whose own data is unchanged keeps the SAME object identity across renders', async () => {
    const data = makeData('Alpha');
    const { rerender } = renderCanvas(data);
    await waitFor(() => expect(capturedNodes.length).toBeGreaterThan(0));
    const before = capturedNodes[capturedNodes.length - 1].find((n) => n.id === 'w1:j1');
    expect(before).toBeDefined();

    rerender(
      <MantineProvider>
        <ReactFlowProvider>
          <Canvas data={makeData('Alpha')} />
        </ReactFlowProvider>
      </MantineProvider>,
    );
    await new Promise((r) => setTimeout(r, 0));

    const after = capturedNodes[capturedNodes.length - 1].find((n) => n.id === 'w1:j1');
    expect(after).toBeDefined();
    expect(after).toBe(before);
  });

  it("a node whose own data DID change gets a fresh object (status: running -> done)", async () => {
    const data = makeData('Alpha', { status: 'running' });
    const { rerender } = renderCanvas(data);
    await waitFor(() => expect(capturedNodes.length).toBeGreaterThan(0));
    const before = capturedNodes[capturedNodes.length - 1].find((n) => n.id === 'w1:j1');

    rerender(
      <MantineProvider>
        <ReactFlowProvider>
          <Canvas data={makeData('Alpha', { status: 'done' })} />
        </ReactFlowProvider>
      </MantineProvider>,
    );
    await new Promise((r) => setTimeout(r, 0));

    const after = capturedNodes[capturedNodes.length - 1].find((n) => n.id === 'w1:j1');
    expect(after).not.toBe(before);
  });

  it('D: viewport is preserved across a data-only update (no remount back to the "Laying out…" state)', async () => {
    const data = makeData('Alpha');
    const { rerender } = renderCanvas(data);
    await waitFor(() => expect(screen.queryByText('Laying out…')).toBeNull());

    rerender(
      <MantineProvider>
        <ReactFlowProvider>
          <Canvas data={makeData('Alpha', { status: 'done' })} />
        </ReactFlowProvider>
      </MantineProvider>,
    );
    await new Promise((r) => setTimeout(r, 0));

    expect(screen.queryByText('Laying out…')).toBeNull();
  });
});
