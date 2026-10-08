import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { useAttention } from './useAttention';

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

const item1 = {
  jobId: 'j1',
  conversationId: 1,
  conversationTitle: 'Lead',
  jobTitle: 'Durable2',
  status: 'failed',
  reason: 'Job failed',
  at: 1,
};

const item2 = {
  jobId: 'j2',
  conversationId: 99,
  conversationTitle: 'Other',
  jobTitle: 'Something',
  status: 'blocked',
  reason: 'needs you',
  at: 2,
};

describe('useAttention', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    FakeEventSource.instances = [];
  });

  it('fetches the attention queue on mount and exposes the items', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ items: [item1] }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const { result, unmount } = renderHook(() => useAttention());

    await waitFor(() => expect(result.current.items).toEqual([item1]));
    expect(result.current.loading).toBe(false);
    expect(fetchMock).toHaveBeenCalledWith('/api/attention');

    unmount();
  });

  it('replaces the item list when an {type:"attention"} frame arrives', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => ({ items: [] }) }));

    const { result, unmount } = renderHook(() => useAttention());
    await waitFor(() => expect(result.current.items).toEqual([]));

    const instance = FakeEventSource.instances[0];
    instance.emitMessage({ type: 'attention', items: [item1] });

    await waitFor(() => expect(result.current.items).toEqual([item1]));

    unmount();
  });

  it('applies an attention frame for a different conversation too (no filtering)', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => ({ items: [] }) }));

    const { result, unmount } = renderHook(() => useAttention());
    await waitFor(() => expect(result.current.items).toEqual([]));

    const instance = FakeEventSource.instances[0];
    instance.emitMessage({ type: 'attention', items: [item2] });

    await waitFor(() => expect(result.current.items).toEqual([item2]));

    unmount();
  });

  it('does not clobber attention state on a {type:"plan"} frame', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: async () => ({ items: [item1] }) }));

    const { result, unmount } = renderHook(() => useAttention());
    await waitFor(() => expect(result.current.items).toEqual([item1]));

    const instance = FakeEventSource.instances[0];
    instance.emitMessage({ type: 'plan', conversation: 1, jobs: [] });

    await new Promise((r) => setTimeout(r, 0));
    expect(result.current.items).toEqual([item1]);

    unmount();
  });
});
