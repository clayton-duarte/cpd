// The browser-side live API (web/README.md "`web/src/api/live.ts`", hs-d819): lists sessions,
// subscribes to one Session's snapshot frames over EventSource and sends one message. `fetch`
// and the EventSource factory are injected; a test never opens a real EventSource or the network.
//
// Stub: `createLiveApi` throws `not implemented` until the impl Job fills it in — this is what
// makes the live tests RED.
import type { SnapshotFrame } from "../fold/fold";
import type { LiveSession } from "./live-server";

export type { LiveSession };

/** The browser-side API the live Chat consumes. */
export interface LiveApi {
  sessions(): Promise<readonly LiveSession[]>;
  /** Subscribes to one Session's snapshot frames; the returned closure closes the stream. */
  subscribe(id: string, onFrame: (frame: SnapshotFrame) => void): () => void;
  /** Sends one message; always posts the fixed one-line reason. */
  send(id: string, text: string): Promise<void>;
}

/** `createLiveApi`'s injected boundaries: `fetch` and `EventSource` are replaced in tests; never real in a test. */
export interface LiveApiDeps {
  token: string;
  /** Same origin by default (""). */
  baseUrl?: string;
  fetch?: typeof fetch;
  EventSource?: typeof EventSource;
}

/** Builds the browser live API (STUB). */
export function createLiveApi(deps: LiveApiDeps): LiveApi {
  void deps;
  throw new Error("not implemented");
}
