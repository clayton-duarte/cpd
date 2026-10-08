import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { usePlan } from './usePlan';
import type { ConversationId } from './types';

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  close = vi.fn();

  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }

  emitMessage(data: unknown) {
    this.onmessage?.({ data: JSON.stringify(data) } as MessageEvent);
  }
}

describe('usePlan', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    FakeEventSource.instances = [];
  });

  it('fetches the plan on mount and exposes the jobs', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ jobs: [{ id: 'j1', title: 'Build', status: 'done', needs: [] }] }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const { result, unmount } = renderHook(() => usePlan(1 as ConversationId));

    await waitFor(() =>
      expect(result.current.jobs).toEqual([{ id: 'j1', title: 'Build', status: 'done', needs: [] }]),
    );
    expect(fetchMock).toHaveBeenCalledWith('/api/plan?conversation=1');

    unmount();
  });

  it('replaces the job list when a {type:"plan"} frame arrives for the watched conversation', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => ({ jobs: [] }) }));

    const { result, unmount } = renderHook(() => usePlan(1 as ConversationId));
    await waitFor(() => expect(result.current.jobs).toEqual([]));

    const instance = FakeEventSource.instances[0];
    instance.emitMessage({
      type: 'plan',
      conversation: 1,
      jobs: [{ id: 'j2', title: 'Deploy', status: 'queued', needs: [] }],
    });

    await waitFor(() =>
      expect(result.current.jobs).toEqual([{ id: 'j2', title: 'Deploy', status: 'queued', needs: [] }]),
    );

    unmount();
  });

  it('ignores a {type:"plan"} frame for a different conversation', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => ({ jobs: [] }) }));

    const { result, unmount } = renderHook(() => usePlan(1 as ConversationId));
    await waitFor(() => expect(result.current.jobs).toEqual([]));

    const instance = FakeEventSource.instances[0];
    instance.emitMessage({
      type: 'plan',
      conversation: 99,
      jobs: [{ id: 'other', title: 'Not ours', status: 'queued', needs: [] }],
    });

    // Give the listener a tick to (not) fire, then assert the list is still empty.
    await new Promise((r) => setTimeout(r, 0));
    expect(result.current.jobs).toEqual([]);

    unmount();
  });

  it('tears down the stream subscription on unmount', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => ({ jobs: [] }) }));

    const { unmount } = renderHook(() => usePlan(1 as ConversationId));
    const instance = FakeEventSource.instances[0];
    expect(instance).toBeDefined();
    expect(instance.close).not.toHaveBeenCalled();

    unmount();

    expect(instance.close).toHaveBeenCalledTimes(1);
  });
});
