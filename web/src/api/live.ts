// The browser-side live API (web/README.md "`web/src/api/live.ts`", hs-d819): lists sessions,
// subscribes to one Session's snapshot frames over EventSource and sends one message. `fetch`
// and the EventSource factory are injected; a test never opens a real EventSource or the network.
import type { SnapshotFrame } from "../fold/fold";
import type { LiveSession, InboxItem, LivePlan, LiveContext } from "./live-server";

export type { LiveSession, InboxItem, LivePlan, LiveContext };

/** The fixed one-line reason every browser send carries (web/README.md). */
export const SEND_UNBLOCKS = "sent from the cpd web chat";

/** The browser-side API the live Chat consumes. */
export interface LiveApi {
  sessions(): Promise<readonly LiveSession[]>;
  /** The open Inbox items the right rail renders (web/README.md "The live Inbox in the right rail"). Optional so the Chat-only fakes need not supply it. */
  inbox?(): Promise<readonly InboxItem[]>;
  /** The Plans the left rail's Plan tree draws (web/README.md "Where the numbers come from"). Optional so the Chat-only fakes need not supply it. */
  plans?(): Promise<readonly LivePlan[]>;
  /** The Contexts the left rail's Context tree draws (web/README.md "Context tree rows"). Optional so the Chat-only fakes need not supply it. */
  contexts?(): Promise<readonly LiveContext[]>;
  /** Subscribes to one Session's snapshot frames; the returned closure closes the stream. */
  subscribe(id: string, onFrame: (frame: SnapshotFrame) => void): () => void;
  /** Sends one message; always posts the fixed one-line reason. */
  send(id: string, text: string): Promise<void>;
  /**
   * One page of older turns, ending just above `before` (the oldest id the caller holds); with no
   * `before`, the newest page. The live frame carries only the tail, so this is how the rest of a
   * long Session is read.
   */
  history?(id: string, before?: string, limit?: number): Promise<LiveHistoryPage>;
}

/** One page of a Session's older turns, with whether anything precedes it. */
export interface LiveHistoryPage {
  messages: readonly unknown[];
  more: boolean;
}

/** `createLiveApi`'s injected boundaries: `fetch` and `EventSource` are replaced in tests; never real in a test. */
export interface LiveApiDeps {
  token: string;
  /** Same origin by default (""). */
  baseUrl?: string;
  fetch?: typeof fetch;
  EventSource?: typeof EventSource;
}

/** Builds the browser live API. */
export function createLiveApi(deps: LiveApiDeps): LiveApi {
  const baseUrl = deps.baseUrl ?? "";
  const fetchImpl = deps.fetch ?? globalThis.fetch;
  const Source = deps.EventSource ?? globalThis.EventSource;

  return {
    async sessions(): Promise<readonly LiveSession[]> {
      const response = await fetchImpl(`${baseUrl}/__cpd/sessions`, {
        headers: { "x-cpd-token": deps.token },
      });
      const payload = (await response.json()) as { sessions?: readonly LiveSession[] };
      return payload.sessions ?? [];
    },

    async inbox(): Promise<readonly InboxItem[]> {
      const response = await fetchImpl(`${baseUrl}/__cpd/inbox`, {
        headers: { "x-cpd-token": deps.token },
      });
      const payload = (await response.json()) as { items?: readonly InboxItem[] };
      return payload.items ?? [];
    },

    async plans(): Promise<readonly LivePlan[]> {
      const response = await fetchImpl(`${baseUrl}/__cpd/plans`, {
        headers: { "x-cpd-token": deps.token },
      });
      const payload = (await response.json()) as { plans?: readonly LivePlan[] };
      return payload.plans ?? [];
    },

    async contexts(): Promise<readonly LiveContext[]> {
      const response = await fetchImpl(`${baseUrl}/__cpd/contexts`, {
        headers: { "x-cpd-token": deps.token },
      });
      const payload = (await response.json()) as { contexts?: readonly LiveContext[] };
      return payload.contexts ?? [];
    },

    subscribe(id: string, onFrame: (frame: SnapshotFrame) => void): () => void {
      // An EventSource cannot set headers, so the token travels in the query.
      const url = `${baseUrl}/__cpd/sessions/${encodeURIComponent(id)}/frames?token=${encodeURIComponent(deps.token)}`;
      const source = new Source(url);
      source.addEventListener("message", (event) => {
        onFrame(JSON.parse((event as MessageEvent).data) as SnapshotFrame);
      });
      return () => {
        source.close();
      };
    },

    async history(id: string, before?: string, limit = 40): Promise<LiveHistoryPage> {
      const query = new URLSearchParams({ limit: String(limit) });
      if (before !== undefined) query.set("before", before);
      const response = await fetchImpl(
        `${baseUrl}/__cpd/sessions/${encodeURIComponent(id)}/transcript?${query.toString()}`,
        { headers: { "x-cpd-token": deps.token } },
      );
      const payload = (await response.json()) as {
        messages?: readonly unknown[];
        more?: boolean;
      };
      return { messages: payload.messages ?? [], more: payload.more === true };
    },

    async send(id: string, text: string): Promise<void> {
      const response = await fetchImpl(`${baseUrl}/__cpd/sessions/${encodeURIComponent(id)}/send`, {
        method: "POST",
        headers: { "content-type": "application/json", "x-cpd-token": deps.token },
        body: JSON.stringify({ text, unblocks: SEND_UNBLOCKS }),
      });
      if (!response.ok) throw new Error(`send failed: ${response.status}`);
    },
  };
}
