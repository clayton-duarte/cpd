import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { useEngineStream } from './useEngineStream';

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

    instance.emitMessage({ type: 'messages', messages: [{ role: 'assistant', content: 'hi' }] });
    await waitFor(() =>
      expect(result.current.messages).toEqual([{ role: 'assistant', content: 'hi' }]),
    );

    unmount();
    expect(instance.close).toHaveBeenCalled();
  });
});
