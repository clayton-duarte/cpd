// The host-frame translation of the dev live adapter (web/README.md "`web/src/translate.ts` — the
// frame translation", hs-d819/chat-host). It folds host frames onto the same `TranscriptMessage[]`
// the CLI path produces (the fold of docs/contract.md §6.20, in TS), so a durable Session read over
// the socket renders exactly like one read through `cpd session tail`.
import { applyImmutable } from "@earendil-works/chord/delta";
import type { TranscriptMessage } from "./transcript.ts";
import type { HostFrame } from "./api/host.ts";

/** The fold state: the transcript so far and the cursor of the last applied frame. */
export interface HostState {
  /** The replica ConversationView; undefined before the first `view` frame. */
  view?: unknown;
  messages: TranscriptMessage[];
  cursor: { epoch: string; seq: number } | undefined;
  /** The id of the last entry whose messages were appended. */
  lastEntryId?: number;
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
export function appendEntry(messages: TranscriptMessage[], entry: unknown): void {
  if (typeof entry !== "object" || entry === null) return;
  const model = (entry as DurableEntry).model;
  if (!Array.isArray(model)) return;
  for (const message of model) {
    const rendered = messageOf(message, `host-${messages.length + 1}`);
    if (rendered !== undefined) messages.push(rendered);
  }
}

/** The entries of a view with id greater than `after`, in order. */
function newEntries(view: unknown, after: number | undefined): Array<{ id: number; entry: unknown }> {
  const entries = (view as { entries?: unknown } | null)?.entries;
  if (!Array.isArray(entries)) return [];
  const out: Array<{ id: number; entry: unknown }> = [];
  for (const entry of entries) {
    const id = (entry as { id?: unknown } | null)?.id;
    if (typeof id !== "number") continue;
    if (after === undefined || id > after) out.push({ id, entry });
  }
  return out;
}

/** Applies one frame; pure: returns the next state. Throws on an ops frame before any view or an op the delta rules refuse. */
export function applyHostFrame(state: HostState, frame: HostFrame): HostState {
  switch (frame.type) {
    case "view":
    case "ops": {
      let view: unknown;
      if (frame.type === "view") view = frame.view;
      else {
        if (state.view === undefined) throw new Error("ops frame before any view");
        view = applyImmutable(state.view as never, frame.ops as never);
      }
      const messages = [...state.messages];
      let lastEntryId = state.lastEntryId;
      for (const { id, entry } of newEntries(view, state.lastEntryId)) {
        appendEntry(messages, entry);
        lastEntryId = id;
      }
      const next: HostState = {
        view,
        messages,
        cursor: { epoch: frame.epoch, seq: frame.seq },
      };
      if (lastEntryId !== undefined) next.lastEntryId = lastEntryId;
      return next;
    }
    default:
      // welcome, attached, host_state, error and bye leave the state unchanged.
      return state;
  }
}

/** Applies frames in order; the fold of the frames stream and the translation tests. */
export function translateFrames(state: HostState, frames: readonly HostFrame[]): HostState {
  let next = state;
  for (const frame of frames) next = applyHostFrame(next, frame);
  return next;
}
