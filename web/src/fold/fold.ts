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
  if (frame.type === "snapshot") {
    // Rule 3: a snapshot frame replaces the whole view; nothing survives it.
    const snapshot = frame.snapshot as { messages?: readonly ChatMessage[] } | null | undefined;
    return {
      epoch: frame.epoch,
      seq: frame.seq,
      messages: snapshot?.messages ?? [],
      cursor: { epoch: frame.epoch, seq: frame.seq },
    };
  }

  // Rule 1: apply each AgentEvent of the batch in order.
  let messages = view.messages;
  for (const event of frame.events) {
    messages = applyEvent(messages, event);
  }
  // Rule 4: the cursor resumes to the frame's epoch/seq.
  return { epoch: frame.epoch, seq: frame.seq, messages, cursor: { epoch: frame.epoch, seq: frame.seq } };
}

/** Applies one `AgentEvent` (rules 1-2): start/update set text, end replaces the whole message. */
function applyEvent(messages: readonly ChatMessage[], event: ChatEvent): readonly ChatMessage[] {
  switch (event.kind) {
    case "message_start":
      return upsert(messages, { id: event.id, role: event.role, text: event.text, pending: true });
    case "message_update": {
      const existing = messages.find((m) => m.id === event.id);
      const role = existing?.role ?? "assistant";
      return upsert(messages, { id: event.id, role, text: event.text, pending: true });
    }
    case "message_end": {
      const existing = messages.find((m) => m.id === event.id);
      const role = existing?.role ?? "assistant";
      // Rule 2: the whole message is replaced at message_end, never concatenated.
      return upsert(messages, { id: event.id, role, text: event.text, pending: false });
    }
  }
}

/** Replaces the message with `id`, or appends it. */
function upsert(messages: readonly ChatMessage[], message: ChatMessage): readonly ChatMessage[] {
  const index = messages.findIndex((m) => m.id === message.id);
  if (index === -1) return [...messages, message];
  const next = [...messages];
  next[index] = message;
  return next;
}
