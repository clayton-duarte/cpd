import { useEffect, useRef, useState } from 'react';
import { getPlan } from './client';
import { useEngineStream, onPlanSignal } from './useEngineStream';
import type { ConversationId, PlanJob } from './types';

type Status = 'connecting' | 'open' | 'closed';

/**
 * Fetches the plan over HTTP on mount / when `conversation` changes (closing the gap in the SSE
 * handshake, which never pushes an initial `{type:"plan"}` frame), then keeps it live via the
 * existing per-conversation stream (H1's `useEngineStream`). Deliberately does NOT open a second
 * `EventSource` -- it rides the same refcounted connection `useEngineStream` already owns, and
 * only adds a plan-frame subscription on top.
 */
export function usePlan(conversation?: ConversationId): { jobs: PlanJob[]; status: Status } {
  const [jobs, setJobs] = useState<PlanJob[]>([]);
  const conversationRef = useRef(conversation);
  conversationRef.current = conversation;

  // Keeps exactly one EventSource per conversation alive (H1's refcounted store) and exposes its
  // connection status. We don't use its `messages` field here.
  const { status } = useEngineStream(conversation);

  useEffect(() => {
    let cancelled = false;
    if (conversation === undefined) {
      setJobs([]);
      return;
    }
    getPlan(conversation).then((response) => {
      if (!cancelled) setJobs(response.jobs);
    });
    return () => {
      cancelled = true;
    };
  }, [conversation]);

  useEffect(() => {
    return onPlanSignal((frameConversation, frameJobs) => {
      if (conversationRef.current === undefined) return;
      if (frameConversation !== conversationRef.current) return;
      setJobs(frameJobs);
    });
  }, []);

  return { jobs, status };
}
