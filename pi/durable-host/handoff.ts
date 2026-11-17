import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { buildSessionProjection, convertToLlm, parseSessionEntries, type SessionEntry } from "@earendil-works/pi-coding-agent";
import type { Message } from "@earendil-works/pi-ai";
import type { JsonValue as DurableJson } from "@earendil-works/chord";
import type { ConversationId } from "@earendil-works/pi-durable";
import type { Tx } from "@earendil-works/pi-durable";

export type JsonValue = null | boolean | number | string | readonly JsonValue[] | { readonly [key: string]: JsonValue };
export type NativeEntry = { readonly id: string; readonly parentId: string | null; readonly type: string; readonly timestamp: number; readonly raw: { readonly [key: string]: JsonValue } };
export type DurableModel = { readonly provider: string; readonly modelId: string };
export type NativeSnapshot = {
  readonly cpdSessionId: string; readonly sourcePath: string; readonly sourceBytesBase64: string; readonly sourceSha256: string;
  readonly header: { readonly [key: string]: JsonValue }; readonly entries: readonly NativeEntry[]; readonly leafId: string | null;
  readonly tree: readonly { readonly id: string; readonly parentId: string | null }[];
  readonly activeAgent: { readonly model: DurableModel; readonly thinkingLevel: string; readonly cwd: string };
  readonly codingAgentVersion: "1.5.2"; readonly codingAgentPiAiVersion: "1.5.2"; readonly durableVersion: "1.5.0"; readonly durablePiAiVersion: "1.5.0";
};
export type PublicTx = Pick<Tx, "appendEntry">;
export type DurableStageSink = {
  readonly versions: { readonly durable: string; readonly durablePiAi: string; readonly codingAgent: string; readonly codingAgentPiAi: string };
  rootAbsent(cpdSessionId: string): Promise<boolean>;
  createRoot(args: { readonly cpdSessionId: string; readonly agent: NativeSnapshot["activeAgent"]; readonly init: (tx: PublicTx, conversationId: string) => Promise<void> }): Promise<{ readonly conversationId: string }>;
};
export type StageResult =
  | { readonly status: "staged"; readonly cpdSessionId: string; readonly durableConversationId: string; readonly sourceSha256: string; readonly sourceToDurableEntryId: Readonly<Record<string, string>>; readonly activeProjectionSourceIds: readonly string[]; readonly leafSourceId: string | null; readonly importVersion: "cpd-native-transcript/1"; readonly stagedOnly: true }
  | { readonly status: "refused"; readonly cpdSessionId: string; readonly sourceSha256: string; readonly code: "invalid-snapshot" | "identity-mismatch" | "unsupported-entry" | "unsupported-extension" | "invalid-tree" | "invalid-projection" | "invalid-message" | "version-incompatible" | "sink-incompatible" | "root-exists" | "sink-failed"; readonly message: string; readonly sourceEntryId?: string };
function mutableJson(value: JsonValue): DurableJson {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(mutableJson);
  return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, mutableJson(v)]));
}

// Only the simple, portable message variants are accepted. Optional provider-specific
// state is deliberately refused rather than assumed compatible across pi-ai versions.
function durableMessage(value: unknown): value is Message {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const m = value as Record<string, unknown>;
  const keys = (allowed: string[]) => Object.keys(m).every((k) => allowed.includes(k));
  const content = (v: unknown, assistant = false): boolean => typeof v === "string" ? !assistant : Array.isArray(v) && v.every((part: unknown) => {
    if (!part || typeof part !== "object" || Array.isArray(part)) return false;
    const p = part as Record<string, unknown>;
    if (p.type === "text") return typeof p.text === "string" && Object.keys(p).every((k) => ["type", "text"].includes(k));
    if (p.type === "image") return !assistant && typeof p.data === "string" && typeof p.mimeType === "string" && Object.keys(p).every((k) => ["type", "data", "mimeType"].includes(k));
    if (p.type === "toolCall") return assistant && typeof p.id === "string" && typeof p.name === "string" && !!p.arguments && typeof p.arguments === "object" && !Array.isArray(p.arguments) && Object.keys(p).every((k) => ["type", "id", "name", "arguments"].includes(k));
    return false;
  });
  if (typeof m.timestamp !== "number" || !Number.isFinite(m.timestamp)) return false;
  if (m.role === "user") return keys(["role", "content", "timestamp"]) && content(m.content);
  if (m.role === "toolResult") return keys(["role", "toolCallId", "toolName", "content", "isError", "timestamp"]) && typeof m.toolCallId === "string" && typeof m.toolName === "string" && typeof m.isError === "boolean" && Array.isArray(m.content) && content(m.content);
  if (m.role !== "assistant" || !keys(["role", "content", "api", "provider", "model", "usage", "stopReason", "timestamp"]) || !Array.isArray(m.content) || !content(m.content, true) || typeof m.api !== "string" || typeof m.provider !== "string" || typeof m.model !== "string" || !["stop", "length", "toolUse", "error", "aborted"].includes(String(m.stopReason))) return false;
  const u = m.usage as Record<string, unknown> | null;
  if (!u || typeof u !== "object" || Array.isArray(u) || Object.keys(u).some((k) => !["input", "output", "cacheRead", "cacheWrite", "totalTokens", "cost"].includes(k))) return false;
  const c = u.cost as Record<string, unknown> | null;
  return ["input", "output", "cacheRead", "cacheWrite", "totalTokens"].every((k) => typeof u[k] === "number" && Number.isFinite(u[k])) && !!c && typeof c === "object" && !Array.isArray(c) && Object.keys(c).every((k) => ["input", "output", "cacheRead", "cacheWrite", "total"].includes(k)) && ["input", "output", "cacheRead", "cacheWrite", "total"].every((k) => typeof c[k] === "number" && Number.isFinite(c[k]));
}

export async function stageNativeConversation(input: NativeSnapshot, sink: DurableStageSink): Promise<StageResult> {
  const refuse = (code: Extract<StageResult, { status: "refused" }>["code"], message: string, sourceEntryId?: string): StageResult => ({ status: "refused", cpdSessionId: input.cpdSessionId, sourceSha256: input.sourceSha256, code, message, ...(sourceEntryId === undefined ? {} : { sourceEntryId }) });
  const versions = { durable: "1.5.0", durablePiAi: "1.5.0", codingAgent: "1.5.2", codingAgentPiAi: "1.5.2" };
  if (!isDeepStrictEqual(sink.versions, versions) || input.durableVersion !== versions.durable || input.durablePiAiVersion !== versions.durablePiAi || input.codingAgentVersion !== versions.codingAgent || input.codingAgentPiAiVersion !== versions.codingAgentPiAi) return refuse("version-incompatible", "Unsupported package versions");
  let parsed: ReturnType<typeof parseSessionEntries>;
  try {
    const bytes = Buffer.from(input.sourceBytesBase64, "base64");
    if (bytes.toString("base64") !== input.sourceBytesBase64 || createHash("sha256").update(bytes).digest("hex") !== input.sourceSha256) throw Error("digest");
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    if (!text.endsWith("\n")) throw Error("incomplete line");
    const lines = text.trimEnd().split(/\r?\n/).map((line) => JSON.parse(line) as unknown);
    if (!isDeepStrictEqual(lines, [input.header, ...input.entries.map((e) => e.raw)])) throw Error("snapshot differs from bytes");
    parsed = parseSessionEntries(text);
    if (parsed.length !== lines.length) throw Error("invalid source");
  } catch { return refuse("invalid-snapshot", "Invalid source bytes, digest or JSONL"); }
  if (!input.cpdSessionId || !input.sourcePath || input.header.type !== "session" || input.header.version !== 3 || typeof input.header.id !== "string" || !input.header.id || typeof input.header.cwd !== "string" || input.header.cwd !== input.activeAgent.cwd || !input.activeAgent.model.provider || !input.activeAgent.model.modelId || !["off", "minimal", "low", "medium", "high", "xhigh"].includes(input.activeAgent.thinkingLevel)) return refuse("invalid-snapshot", "Invalid header or active agent");
  const entries = new Map<string, NativeEntry>();
  const allowed = new Set(["message", "custom_message"]);
  for (const entry of input.entries) {
    if (!entry.id || entries.has(entry.id) || (entry.parentId !== null && !entries.has(entry.parentId)) || entry.raw.id !== entry.id || entry.raw.parentId !== entry.parentId || entry.raw.type !== entry.type || typeof entry.raw.timestamp !== "string" || Date.parse(entry.raw.timestamp) !== entry.timestamp) return refuse("invalid-tree", "Invalid entry identity, order or parent", entry.id);
    if (!allowed.has(entry.type)) return refuse(entry.type === "custom" ? "unsupported-extension" : "unsupported-entry", "Unsupported source entry", entry.id);
    if (entry.type === "message" && !durableMessage(entry.raw.message)) return refuse("invalid-message", "Unsupported source message", entry.id);
    if (entry.type === "custom_message" && (entry.raw.customType !== "cpd" || typeof entry.raw.content !== "string" || typeof entry.raw.display !== "boolean" || !entry.raw.details || typeof entry.raw.details !== "object" || Array.isArray(entry.raw.details))) return refuse("unsupported-extension", "Unsupported custom message", entry.id);
    entries.set(entry.id, entry);
  }
  if (input.tree.length !== input.entries.length || input.tree.some((e, i) => e.id !== input.entries[i]?.id || e.parentId !== input.entries[i]?.parentId) || (input.leafId !== null && !entries.has(input.leafId)) || (input.leafId === null && entries.size !== 0)) return refuse("invalid-tree", "Tree or leaf disagrees with source");
  let projection: ReturnType<typeof buildSessionProjection>;
  try { projection = buildSessionProjection(parsed.slice(1) as SessionEntry[], input.leafId); } catch { return refuse("invalid-projection", "Cannot project source"); }
  if (projection.thinkingLevel !== input.activeAgent.thinkingLevel || (projection.model !== null && !isDeepStrictEqual(projection.model, input.activeAgent.model))) return refuse("invalid-projection", "Active model differs from projection");
  const projected: { id: string; model: Message[] }[] = [];
  const pending = new Map<string, string>();
  for (const item of projection.entries) {
    let converted: unknown[];
    try { converted = convertToLlm(item.messages); } catch { return refuse("invalid-projection", "Cannot convert projection", item.sourceEntry.id); }
    if (!converted.every(durableMessage)) return refuse("invalid-message", "Unsupported durable message", item.sourceEntry.id);
    for (const m of converted) {
      if (m.role === "assistant") for (const part of m.content) if (part.type === "toolCall") { if (pending.has(part.id)) return refuse("invalid-projection", "Duplicate tool call", item.sourceEntry.id); pending.set(part.id, part.name); }
      if (m.role === "toolResult") { if (pending.get(m.toolCallId) !== m.toolName) return refuse("invalid-projection", "Unmatched tool result", item.sourceEntry.id); pending.delete(m.toolCallId); }
    }
    if (converted.length) projected.push({ id: item.sourceEntry.id, model: converted });
  }
  if (pending.size) return refuse("invalid-projection", "Unfinished tool calls");
  const map: Record<string, string> = Object.create(null) as Record<string, string>;
  try {
    if (!await sink.rootAbsent(input.cpdSessionId)) return refuse("root-exists", "Root already exists");
    const root = await sink.createRoot({ cpdSessionId: input.cpdSessionId, agent: input.activeAgent, init: async (tx, conversationId) => {
      const id = (Number.isSafeInteger(Number(conversationId)) ? Number(conversationId) : conversationId) as unknown as ConversationId;
      for (const [ordinal, entry] of input.entries.entries()) {
        const record = await tx.appendEntry(id, { kind: "cpd.native-entry", model: [], data: { importVersion: "cpd-native-transcript/1", cpdSessionId: input.cpdSessionId, sourcePath: input.sourcePath, sourceSha256: input.sourceSha256, ordinal, sourceId: entry.id, parentId: entry.parentId, timestamp: entry.timestamp, type: entry.type, raw: mutableJson(entry.raw) } });
        map[entry.id] = String(record.id);
      }
      await tx.appendEntry(id, { kind: "cpd.native-header", model: [], data: { header: mutableJson(input.header), sourceBytesBase64: input.sourceBytesBase64, sourceSha256: input.sourceSha256, leafId: input.leafId, tree: mutableJson(input.tree), versions } });
      for (const item of projected) await tx.appendEntry(id, { kind: "cpd.native-projection", model: item.model, data: { sourceId: item.id } });
      await tx.appendEntry(id, { kind: "cpd.native-map", model: [], data: { sourceToDurableEntryId: map, tree: mutableJson(input.tree), leafId: input.leafId } });
    } });
    return { status: "staged", cpdSessionId: input.cpdSessionId, durableConversationId: root.conversationId, sourceSha256: input.sourceSha256, sourceToDurableEntryId: { ...map }, activeProjectionSourceIds: projected.map((p) => p.id), leafSourceId: input.leafId, importVersion: "cpd-native-transcript/1", stagedOnly: true };
  } catch (e) { return refuse("sink-failed", e instanceof Error ? e.message : String(e)); }
}
