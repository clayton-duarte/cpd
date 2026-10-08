import { useEffect, useState } from 'react';
import { getAttention } from './client';
import { useEngineStream, onAttentionSignal } from './useEngineStream';
import type { AttentionItem } from './types';

/**
 * Fetches the global attention queue over HTTP on mount, then keeps it live via the existing
 * shared stream (H1's `useEngineStream`) -- deliberately does NOT open a second `EventSource`.
 * Unlike `usePlan`, attention is global (not per-conversation), so every `{type:"attention"}`
 * frame applies regardless of which conversation is selected.
 */
export function useAttention(): { items: AttentionItem[]; loading: boolean } {
  const [items, setItems] = useState<AttentionItem[]>([]);
  const [loading, setLoading] = useState(true);

  // Rides the existing root stream connection; we don't use its `messages`/`status` fields here.
  useEngineStream();

  useEffect(() => {
    let cancelled = false;
    getAttention().then((response) => {
      if (!cancelled) {
        setItems(response.items);
        setLoading(false);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    return onAttentionSignal((frameItems) => {
      setItems(frameItems);
    });
  }, []);

  return { items, loading };
}
