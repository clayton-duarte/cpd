// The TS chat fold (docs/research/web-client-api.md §5, hs-d772): apply each `AgentEvent` of
// an `events` frame in order onto the snapshot state (rule 1); a whole message arrives at
// `message_start` and is replaced at `message_end` (rule 2); a `snapshot` frame replaces the
// whole view (rule 3); the cursor resumes epoch/seq (rule 4).
//
// Stub: `fold` throws `not implemented` until the impl Job fills it in — this is what makes
// the fold tests RED.
import type { Cursor } from "../api/types";

/** A chat `AgentEvent` folded by the chat bridge (§5.2). */
export type ChatEvent =
  | { kind: "message_start"; id: string; role: "user" | "assistant"; text: string }
  | { kind: "message_update"; id: string; text: string }
  | { kind: "message_end"; id: string; text: string };

/** An `events` frame: one commit of `AgentEvent`s (§1.1, protocol.ts:45). */
export interface EventsFrame {
  type: "events";
  epoch: string;
  seq: number;
  events: readonly ChatEvent[];
}

/** A `snapshot` frame replacing the whole view (§5.3). */
export interface SnapshotFrame {
  type: "snapshot";
  epoch: string;
  seq: number;
  snapshot: unknown;
}

/** One rendered chat message. */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  pending: boolean;
}

/** The client's chat view: a fold over the SnapshotEvent and the applied events. */
export interface ChatView {
  epoch: string;
  seq: number;
  messages: readonly ChatMessage[];
  cursor: Cursor;
}

/** Merges one detected frame onto the chat view (STUB). */
export function fold(view: ChatView, frame: EventsFrame | SnapshotFrame): ChatView {
  void view;
  void frame;
  throw new Error("not implemented");
}
