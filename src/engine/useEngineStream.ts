import { useEffect, useSyncExternalStore } from 'react';
import type { Message, StreamEvent } from './types';

type Status = 'connecting' | 'open' | 'closed';

const BASE = (import.meta.env.VITE_CPD_API as string | undefined) ?? '/api';

let messages: Message[] = [];
let status: Status = 'connecting';
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getMessagesSnapshot(): Message[] {
  return messages;
}

function getStatusSnapshot(): Status {
  return status;
}

let refCount = 0;
let source: EventSource | null = null;

function connect() {
  if (source) return;
  status = 'connecting';
  source = new EventSource(`${BASE}/stream`);

  source.onopen = () => {
    status = 'open';
    emit();
  };

  source.onerror = () => {
    status = 'closed';
    emit();
  };

  source.onmessage = (e: MessageEvent) => {
    const parsed = JSON.parse(e.data) as StreamEvent;
    if (parsed.type === 'messages') {
      messages = parsed.messages;
      emit();
    }
  };
}

function disconnect() {
  source?.close();
  source = null;
}

/**
 * Subscribes to the SSE message stream. The underlying EventSource and its
 * message list are module-level (a single shared store) so getSnapshot can
 * return a cached reference -- returning a fresh array on every call would
 * loop useSyncExternalStore forever.
 */
export function useEngineStream(): { messages: Message[]; status: Status } {
  const streamMessages = useSyncExternalStore(subscribe, getMessagesSnapshot);
  const streamStatus = useSyncExternalStore(subscribe, getStatusSnapshot);

  useEffect(() => {
    refCount += 1;
    if (refCount === 1) connect();
    return () => {
      refCount -= 1;
      if (refCount === 0) disconnect();
    };
  }, []);

  return { messages: streamMessages, status: streamStatus };
}
