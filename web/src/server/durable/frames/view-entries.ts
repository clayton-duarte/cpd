import type { AssistantBlock, AssistantItem, FoldItem, FoldKind, TranscriptAttachment, TranscriptItem } from "../../../protocol/domain.ts";
import { FOLD_KINDS, foldTitle, isoOf, skillsBaseline, skillsMessage, textOf } from "./message-fields.ts";
import { arr, isObj, num, obj, protocolError, str, type Obj } from "./validation.ts";

/** Text and thinking blocks of an assistant message; a live (partial) message may lack fields. */
export function blocksOf(message: Obj, live: boolean): AssistantBlock[] {
  const blocks: AssistantBlock[] = [];
  for (const raw of arr(message.content, "assistant content")) {
    const b = obj(raw, "assistant block");
    if (b.type === "text") blocks.push({ type: "text", text: live && typeof b.text !== "string" ? "" : str(b.text, "text block") });
    else if (b.type === "thinking") {
      const text = live && typeof b.thinking !== "string" ? "" : str(b.thinking, "thinking block");
      if (text !== "") blocks.push({ type: "thinking", text });
    }
  }
  if (typeof message.errorMessage === "string" && message.errorMessage !== "") blocks.push({ type: "text", text: "Error: " + message.errorMessage });
  return blocks;
}

export type ToolBlock = { callId: string; name: string; args: Obj };
export const toolBlocksOf = (message: Obj, live: boolean): ToolBlock[] =>
  arr(message.content, "assistant content")
    .filter((b): b is Obj => isObj(b) && b.type === "toolCall")
    .map((b) => ({
      callId: str(b.id, "toolCall id"),
      name: live && typeof b.name !== "string" ? "" : str(b.name, "toolCall name"),
      args: isObj(b.arguments) ? b.arguments : {},
    }));

export type Parsed =
  | { kind: "item"; item: TranscriptItem }
  | { kind: "assistant"; item?: AssistantItem; tools: ToolBlock[]; tokens?: number }
  | { kind: "result"; callId: string; name: string; result: { text: string; isError: boolean }; patch?: string }
  | { kind: "review"; review: string; status: string }
  | { kind: "none" };

export function parseEntry(rawEntry: unknown): { id: number; parsed: Parsed } {
  const entry = obj(rawEntry, "entry");
  const id = num(entry.id, "entry id");
  const kind = str(entry.kind, "entry kind");
  const messageOf = (role?: string): Obj => {
    const message = obj(arr(entry.model, `entry ${id} model`)[0], `entry ${id} model[0]`);
    if (role !== undefined && message.role !== role) protocolError(`entry ${id}: expected role ${role}, got ${String(message.role)}`);
    return message;
  };
  if (kind === "pi.user" || kind === "cpd-chat") {
    const message = messageOf("user");
    const attachments: TranscriptAttachment[] = Array.isArray(message.content)
      ? message.content
          .filter((b): b is Obj => isObj(b) && b.type === "image")
          .map((b) => ({ kind: "image", mimeType: str(b.mimeType, "image mimeType"), dataBase64: str(b.data, "image data") }))
      : [];
    return { id, parsed: { kind: "item", item: { kind: "user", id: `u:${id}`, ...isoOf(message), text: textOf(message.content), attachments } } };
  }
  if (kind === "pi.assistant") {
    const message = messageOf("assistant");
    const blocks = blocksOf(message, false);
    const usage = isObj(message.usage) ? message.usage : undefined;
    return {
      id,
      parsed: {
        kind: "assistant",
        tools: toolBlocksOf(message, false),
        ...(typeof usage?.totalTokens === "number" ? { tokens: usage.totalTokens } : {}),
        ...(blocks.length > 0
          ? { item: { kind: "assistant", id: `a:${id}`, ...isoOf(message), blocks, streaming: false, ...(typeof message.model === "string" ? { model: message.model } : {}) } }
          : {}),
      },
    };
  }
  if (kind === "pi.tool-result") {
    const message = messageOf("toolResult");
    const details = isObj(message.details) ? message.details : {};
    const patch = typeof details.patch === "string" ? details.patch : typeof details.diff === "string" ? details.diff : undefined;
    return {
      id,
      parsed: {
        kind: "result",
        callId: str(message.toolCallId, "toolCallId"),
        name: str(message.toolName, "toolName"),
        result: { text: textOf(message.content), isError: message.isError === true },
        ...(patch === undefined ? {} : { patch }),
      },
    };
  }
  if (kind === "pi.compaction") {
    const message = messageOf();
    return { id, parsed: { kind: "item", item: { kind: "compaction", id: `c:${id}`, ...isoOf(message), summary: textOf(message.content) } } };
  }
  if (skillsBaseline(entry)) return { id, parsed: { kind: "none" } };
  if ((FOLD_KINDS as readonly string[]).includes(kind)) {
    const message = messageOf();
    if (kind === "cpd-skills") skillsMessage(message);
    const body = textOf(message.content);
    const foldKind = kind as FoldKind;
    const foldData =
      isObj(entry.data) && Object.keys(entry.data).length > 0
        ? entry.data
        : isObj(message.details)
          ? message.details
          : {};
    const fold: FoldItem = {
      kind: "fold",
      id: `f:${id}`,
      ...isoOf(message),
      foldKind,
      title: foldTitle(foldKind, body, foldData),
      body,
      defaultOpen: foldKind === "cpd-plan-summary",
    };
    return { id, parsed: { kind: "item", item: fold } };
  }
  if (kind === "cpd-plan-review" && isObj(entry.data)) {
    const review = entry.data.review;
    const status = entry.data.status;
    if ((typeof review === "string" || typeof review === "number") && typeof status === "string") {
      return { id, parsed: { kind: "review", review: String(review), status } };
    }
  }
  // pi.system, pi.reset, cpd-event, cpd-first-message, cpd-error and any other kind: dropped (CONTRACT 6.4)
  return { id, parsed: { kind: "none" } };
}
