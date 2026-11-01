// The host-frame translation of the dev live adapter (web/README.md "`web/src/translate.ts` — the
// frame translation", hs-d819/chat-host). It folds host frames onto the same `TranscriptMessage[]`
// the CLI path produces (the fold of docs/contract.md §6.20, in TS), so a durable Session read over
// the socket renders exactly like one read through `cpd session tail`.
import type { TranscriptMessage } from "./transcript.ts";
import type { HostFrame } from "./api/host.ts";

/** The fold state: the transcript so far and the cursor of the last applied frame. */
export interface HostState {
  messages: TranscriptMessage[];
  cursor: { epoch: string; seq: number } | undefined;
  /** The in-flight message of a `message_start`, completed by a later `run_end` (rule 2). */
  pending?: unknown;
}

/** The fields of a pi-durable `EntryRecord` this fold reads. */
interface DurableEntry {
  model?: unknown;
}

/** The fields of a pi-ai `Message` this fold reads. */
interface DurableMessage {
  role?: unknown;
  content?: unknown;
  toolName?: unknown;
  isError?: unknown;
}

/** The text of a message's `content`: a plain string, or the join of its `text` blocks (thinking skipped). */
function textOf(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  const parts: string[] = [];
  for (const block of content) {
    if (typeof block !== "object" || block === null) continue;
    const record = block as { type?: unknown; text?: unknown };
    if (record.type === "text" && typeof record.text === "string") parts.push(record.text);
  }
  return parts.join("");
}

/** Renders one pi-ai `Message` like the tail's line: role from the message type, a `toolResult` as `tool <name>`. */
function messageOf(message: unknown, id: string): TranscriptMessage | undefined {
  if (typeof message !== "object" || message === null) return undefined;
  const record = message as DurableMessage;
  switch (record.role) {
    case "user":
      return { id, role: "user", text: textOf(record.content) };
    case "assistant":
      return { id, role: "assistant", text: textOf(record.content) };
    case "system":
      return { id, role: "system", text: textOf(record.content) };
    case "toolResult": {
      const rendered: TranscriptMessage = {
        id,
        role: "tool",
        name: typeof record.toolName === "string" ? record.toolName : "",
        text: textOf(record.content),
      };
      if (record.isError === true) rendered.error = true;
      return rendered;
    }
    default:
      return undefined;
  }
}

/** Appends the model messages of one `EntryRecord`, in order; an entry with no model messages is bookkeeping and adds nothing. */
function appendEntry(messages: TranscriptMessage[], entry: unknown): void {
  if (typeof entry !== "object" || entry === null) return;
  const model = (entry as DurableEntry).model;
  if (!Array.isArray(model)) return;
  for (const message of model) {
    const rendered = messageOf(message, `host-${messages.length + 1}`);
    if (rendered !== undefined) messages.push(rendered);
  }
}

/** Applies one frame; pure: returns the next state. */
export function applyHostFrame(state: HostState, frame: HostFrame): HostState {
  switch (frame.type) {
    case "snapshot": {
      // Rule 1: a snapshot replaces the whole state, never merges.
      const messages: TranscriptMessage[] = [];
      const snapshot = frame.snapshot as { entries?: unknown } | null | undefined;
      if (snapshot !== null && typeof snapshot === "object" && Array.isArray(snapshot.entries)) {
        for (const entry of snapshot.entries) appendEntry(messages, entry);
      }
      return { messages, cursor: { epoch: frame.epoch, seq: frame.seq }, pending: undefined };
    }
    case "events": {
      // Rule 2: apply each AgentEvent in order; message_end and a run_end ending an in-flight message append.
      const messages = [...state.messages];
      let pending = state.pending;
      for (const event of frame.events) {
        if (typeof event !== "object" || event === null) continue;
        const record = event as { type?: unknown; message?: unknown; entry?: unknown };
        if (record.type === "message_start") {
          pending = record.message;
        } else if (record.type === "message_end") {
          appendEntry(messages, record.entry);
          pending = undefined;
        } else if (record.type === "run_end") {
          if (pending !== undefined) appendEntry(messages, { model: [pending] });
          pending = undefined;
        }
      }
      // Rule 3: the cursor advances with the stream frame once its events are applied.
      return { messages, cursor: { epoch: frame.epoch, seq: frame.seq }, pending };
    }
    default:
      // Rule 4: welcome, attached, host_state, error and bye leave the state unchanged.
      return state;
  }
}

/** Applies frames in order; the fold of the frames stream and the translation tests. */
export function translateFrames(state: HostState, frames: readonly HostFrame[]): HostState {
  let next = state;
  for (const frame of frames) next = applyHostFrame(next, frame);
  return next;
}
