import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { useEngineStream } from './useEngineStream';
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

  emitOpen() {
    this.onopen?.();
  }

  emitMessage(data: unknown) {
    this.onmessage?.({ data: JSON.stringify(data) } as MessageEvent);
  }
}

describe('useEngineStream', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    FakeEventSource.instances = [];
  });

  it('starts connecting, opens, updates messages, and closes on unmount', async () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);

    const { result, unmount } = renderHook(() => useEngineStream());

    expect(result.current.status).toBe('connecting');

    const instance = FakeEventSource.instances[0];
    expect(instance).toBeDefined();

    instance.emitOpen();
    await waitFor(() => expect(result.current.status).toBe('open'));

    instance.emitMessage({ type: 'messages', messages: [{ id: 7, role: 'assistant', content: 'hi' }] });
    await waitFor(() =>
      expect(result.current.messages).toEqual([{ id: 7, role: 'assistant', content: 'hi' }]),
    );

    unmount();
    expect(instance.close).toHaveBeenCalled();
  });

  it('subscribes to the given conversation id in the stream URL', () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);

    renderHook(() => useEngineStream(16 as ConversationId));

    const instance = FakeEventSource.instances[0];
    expect(instance.url).toBe('/api/stream?conversation=16');
  });

  it('switching conversation id tears down the old EventSource and opens exactly one new one', () => {
    vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);

    const { rerender } = renderHook(({ id }: { id?: ConversationId }) => useEngineStream(id), {
      initialProps: { id: undefined as ConversationId | undefined },
    });

    expect(FakeEventSource.instances).toHaveLength(1);
    const first = FakeEventSource.instances[0];
    expect(first.url).toBe('/api/stream');
    expect(first.close).not.toHaveBeenCalled();

    rerender({ id: 16 as ConversationId });

    expect(first.close).toHaveBeenCalledTimes(1);
    expect(FakeEventSource.instances).toHaveLength(2);
    const second = FakeEventSource.instances[1];
    expect(second.url).toBe('/api/stream?conversation=16');
    expect(second.close).not.toHaveBeenCalled();
  });
});
