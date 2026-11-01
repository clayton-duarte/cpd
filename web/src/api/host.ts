// The read-only host-socket client of the dev live adapter (web/README.md "`web/src/api/host.ts` —
// the socket client", hs-d819/chat-host). One connect + `hello` + `attach` per `attach()` call, the
// parsed frames out, reconnecting per the README's rule. The node socket is the one external
// boundary: `node:net` in production, a fake in tests. No test opens a real socket.
//
// Stub: `createHostClient` throws `not implemented` until the implementation Job fills it in — this
// is what makes `web/src/api/host.test.ts` RED.
import type { Socket } from "node:net";

/** A frame the host sent: protocol.ts's ToClient, the subset a hello/attach-only reader receives. */
export type HostFrame =
  | { type: "welcome"; session: string; role: "lead" | "manager"; epoch: string }
  | {
      type: "attached";
      req: string;
      mode: "snapshot" | "resume";
      epoch: string;
      from_seq: number;
      head_seq: number;
    }
  | { type: "snapshot"; epoch: string; seq: number; snapshot: unknown }
  | { type: "events"; epoch: string; seq: number; events: unknown[] }
  | { type: "host_state"; state: "idle" | "working" | "waiting" | "blocked" }
  | { type: "error"; code: string; message: string }
  | { type: "bye"; reason: string; reconnect: boolean };

/** The one external boundary: the node socket (node:net in production, replaced in tests). */
export type HostConnect = (path: string) => Promise<Socket>;

/** The read-only socket client: one connect + hello + attach per attach() call, frames out, reconnecting per the rule below. */
export interface HostClient {
  /** Attaches to the Session's host socket and calls onFrame per received frame until the returned closure closes the socket. */
  attach(id: string, onFrame: (frame: HostFrame) => void): () => void;
}

export function createHostClient(_connect: HostConnect): HostClient {
  throw new Error("not implemented");
}
