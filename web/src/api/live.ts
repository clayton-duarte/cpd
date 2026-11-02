// The browser-side live API (web/README.md "`web/src/api/live.ts`", hs-d819): lists sessions,
// subscribes to one Session's snapshot frames over EventSource and sends one message. `fetch`
// and the EventSource factory are injected; a test never opens a real EventSource or the network.
import type { SnapshotFrame } from "../fold/fold";
import type { LiveSession, InboxItem, LivePlan } from "./live-server";

export type { LiveSession, InboxItem, LivePlan };

/** The fixed one-line reason every browser send carries (web/README.md). */
export const SEND_UNBLOCKS = "sent from the cpd web chat";

/** The browser-side API the live Chat consumes. */
export interface LiveApi {
  sessions(): Promise<readonly LiveSession[]>;
  /** The open Inbox items the right rail renders (web/README.md "The live Inbox in the right rail"). Optional so the Chat-only fakes need not supply it. */
  inbox?(): Promise<readonly InboxItem[]>;
  /** The Plans the left rail's Plan tree draws (web/README.md "Where the numbers come from"). Optional so the Chat-only fakes need not supply it. */
  plans?(): Promise<readonly LivePlan[]>;
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

/** `createLiveApi`'s injected boundaries: `fetch`, `WebSocket` and `EventSource` are replaced in tests; never real in a test. */
export interface LiveApiDeps {
  token: string;
  /** Same origin by default (""). */
  baseUrl?: string;
  fetch?: typeof fetch;
  /** The WebSocket factory the stream opens (web/README.md "The WebSocket stream"); the browser default when absent. */
  WebSocket?: typeof WebSocket;
  /** The legacy SSE factory, kept so an injected `EventSource` (an older caller, a test fake) still streams. */
  EventSource?: typeof EventSource;
}

/** Builds the browser live API. */
export function createLiveApi(deps: LiveApiDeps): LiveApi {
  const baseUrl = deps.baseUrl ?? "";
  const fetchImpl = deps.fetch ?? globalThis.fetch;

  /** The route the browser opens: the token travels in the query (a handshake cannot set a header). */
  const wsUrl = (id: string, cursor?: { epoch: string; seq: number }): string => {
    const params = new URLSearchParams({ token: deps.token });
    if (cursor !== undefined) {
      params.set("epoch", cursor.epoch);
      params.set("seq", String(cursor.seq));
    }
    return `${baseUrl}/__cpd/sessions/${encodeURIComponent(id)}/ws?${params.toString()}`;
  };

  /** The WebSocket stream: frames out, and a reconnect that re-attaches with the last cursor. */
  const subscribeSocket = (
    Factory: typeof WebSocket,
    id: string,
    onFrame: (frame: SnapshotFrame) => void,
  ): (() => void) => {
    let socket: WebSocket | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    let cursor: { epoch: string; seq: number } | undefined;
    let attempt = 0;

    const open = (): void => {
      if (stopped) return;
      const next = new Factory(wsUrl(id, cursor));
      socket = next;
      next.addEventListener("message", (event) => {
        const frame = JSON.parse((event as MessageEvent).data) as SnapshotFrame;
        // The cursor carries the last stream frame across a reconnect (web/src/api/host.ts's rule).
        if (typeof frame.epoch === "string" && typeof frame.seq === "number") {
          cursor = { epoch: frame.epoch, seq: frame.seq };
        }
        onFrame(frame);
      });
      next.addEventListener("close", () => {
        if (stopped) return;
        // The host client's back-off: 100 ms doubling to a 1 s cap.
        const delay = Math.min(100 * 2 ** attempt, 1000);
        attempt += 1;
        timer = setTimeout(open, delay);
      });
    };

    open();
    return () => {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      socket?.close();
    };
  };

  /** The legacy SSE stream (an injected `EventSource`), unchanged from before the WebSocket bridge. */
  const subscribeSource = (
    Factory: typeof EventSource,
    id: string,
    onFrame: (frame: SnapshotFrame) => void,
  ): (() => void) => {
    const url = `${baseUrl}/__cpd/sessions/${encodeURIComponent(id)}/frames?token=${encodeURIComponent(deps.token)}`;
    const source = new Factory(url);
    source.addEventListener("message", (event) => {
      onFrame(JSON.parse((event as MessageEvent).data) as SnapshotFrame);
    });
    return () => {
      source.close();
    };
  };

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

    subscribe(id: string, onFrame: (frame: SnapshotFrame) => void): () => void {
      // The bridge opens a WebSocket; an injected `EventSource` (an older caller, a test fake) keeps
      // the SSE stream, and the browser default is a WebSocket either way.
      if (deps.WebSocket !== undefined) return subscribeSocket(deps.WebSocket, id, onFrame);
      if (deps.EventSource !== undefined) return subscribeSource(deps.EventSource, id, onFrame);
      const globalSocket = globalThis.WebSocket;
      if (typeof globalSocket === "function") return subscribeSocket(globalSocket, id, onFrame);
      const globalSource = globalThis.EventSource;
      if (typeof globalSource === "function") return subscribeSource(globalSource, id, onFrame);
      throw new Error("neither WebSocket nor EventSource is available");
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
