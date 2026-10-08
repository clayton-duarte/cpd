import { useEffect, useSyncExternalStore } from 'react';
import type { ConversationId, Message, StreamEvent } from './types';

type Status = 'connecting' | 'open' | 'closed';

const BASE = (import.meta.env.VITE_CPD_API as string | undefined) ?? '/api';

/** Per-conversation stream store. Keyed by conversation id (or 'root' for the default/no-id
 * stream) so each conversation gets its own refcounted EventSource -- switching conversations
 * tears down the previous one rather than leaking it or running two at once. */
type StreamEntry = {
  messages: Message[];
  status: Status;
  listeners: Set<() => void>;
  source: EventSource | null;
  refCount: number;
};

const entries = new Map<string, StreamEntry>();

/** Fired whenever any active stream delivers a `{type:"conversations"}` signal -- the tree
 * section in Sidebar subscribes here to know when to refetch `/api/conversations`. */
const conversationsListeners = new Set<() => void>();

export function onConversationsSignal(listener: () => void): () => void {
  conversationsListeners.add(listener);
  return () => conversationsListeners.delete(listener);
}


function keyFor(conversation: ConversationId | undefined): string {
  return conversation === undefined ? 'root' : String(conversation);
}

function getEntry(key: string): StreamEntry {
  let entry = entries.get(key);
  if (!entry) {
    entry = { messages: [], status: 'connecting', listeners: new Set(), source: null, refCount: 0 };
    entries.set(key, entry);
  }
  return entry;
}

function emit(entry: StreamEntry) {
  for (const listener of entry.listeners) listener();
}

function connect(key: string, conversation: ConversationId | undefined) {
  const entry = getEntry(key);
  if (entry.source) return;
  entry.status = 'connecting';
  const suffix = conversation === undefined ? '' : `?conversation=${conversation}`;
  const source = new EventSource(`${BASE}/stream${suffix}`);
  entry.source = source;

  source.onopen = () => {
    entry.status = 'open';
    emit(entry);
  };

  source.onerror = () => {
    entry.status = 'closed';
    emit(entry);
  };

  source.onmessage = (e: MessageEvent) => {
    const parsed = JSON.parse(e.data) as StreamEvent;
    if (parsed.type === 'messages') {
      entry.messages = parsed.messages;
      emit(entry);
    } else if (parsed.type === 'conversations') {
      for (const listener of conversationsListeners) listener();
    }
  };
}

function disconnect(key: string) {
  const entry = entries.get(key);
  entry?.source?.close();
  if (entry) entry.source = null;
}

/**
 * Subscribes to the SSE message stream for one conversation (root, if `conversation` is
 * undefined). Each conversation id has its own module-level refcounted store, so getSnapshot can
 * return a cached reference per key -- returning a fresh array on every call would loop
 * useSyncExternalStore forever. Changing `conversation` tears down the previous EventSource and
 * opens exactly one new one; it never leaves two live.
 */
export function useEngineStream(conversation?: ConversationId): { messages: Message[]; status: Status } {
  const key = keyFor(conversation);
  const entry = getEntry(key);

  const subscribe = (listener: () => void): (() => void) => {
    entry.listeners.add(listener);
    return () => entry.listeners.delete(listener);
  };

  const streamMessages = useSyncExternalStore(subscribe, () => getEntry(key).messages);
  const streamStatus = useSyncExternalStore(subscribe, () => getEntry(key).status);

  useEffect(() => {
    const activeEntry = getEntry(key);
    activeEntry.refCount += 1;
    if (activeEntry.refCount === 1) connect(key, conversation);
    return () => {
      activeEntry.refCount -= 1;
      if (activeEntry.refCount === 0) disconnect(key);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return { messages: streamMessages, status: streamStatus };
}
