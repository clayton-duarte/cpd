import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import type { ConversationId, EntryDraft, EntryId } from "@earendil-works/pi-durable";
import { stageNativeConversation, type DurableStageSink, type JsonValue, type PublicTx, type NativeEntry, type NativeSnapshot } from "../../durable-host/handoff.ts";

const time = "2026-01-01T00:00:00.000Z";
const header = { type: "session", version: 3, id: "native-id", timestamp: time, cwd: "/work" } as const;
const usage = { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, totalTokens: 3, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } };
function row(id: string, parentId: string | null, extra: { readonly [key: string]: JsonValue }): NativeEntry {
  const raw = { ...extra, id, parentId, timestamp: time };
  return { id, parentId, type: String(extra.type), timestamp: Date.parse(time), raw };
}
const rows = [
  row("u", null, { type: "message", message: { role: "user", content: [{ type: "text", text: "read it" }], timestamp: 1 } }),
  row("a", "u", { type: "message", message: { role: "assistant", content: [{ type: "toolCall", id: "call-1", name: "read", arguments: { path: "a" } }], api: "anthropic-messages", provider: "anthropic", model: "model", usage, stopReason: "toolUse", timestamp: 2 } }),
  row("t", "a", { type: "message", message: { role: "toolResult", toolCallId: "call-1", toolName: "read", content: [{ type: "text", text: "contents" }], isError: false, timestamp: 3 } }),
  row("off", "u", { type: "custom_message", customType: "cpd", content: "notice", display: true, details: { id: 42, kind: "session-message", session: "cpd-session-1", text: "notice", data: { from: null, text: "notice", wake: false }, delivered_to: "cpd-session-1" } }),
  row("z", "t", { type: "message", message: { role: "assistant", content: [{ type: "text", text: "done" }], api: "anthropic-messages", provider: "anthropic", model: "model", usage, stopReason: "stop", timestamp: 4 } }),
];
function snapshot(entries: readonly NativeEntry[] = rows): NativeSnapshot {
  const bytes = Buffer.from([header, ...entries.map((e) => e.raw)].map((x) => JSON.stringify(x)).join("\n") + "\n");
  return { cpdSessionId: "cpd-session-1", sourcePath: "/work/native.jsonl", sourceBytesBase64: bytes.toString("base64"), sourceSha256: createHash("sha256").update(bytes).digest("hex"), header, entries, leafId: "z", tree: entries.map(({ id, parentId }) => ({ id, parentId })), activeAgent: { model: { provider: "anthropic", modelId: "model" }, thinkingLevel: "off", cwd: "/work" }, codingAgentVersion: "1.5.2", codingAgentPiAiVersion: "1.5.2", durableVersion: "1.5.0", durablePiAiVersion: "1.5.0" };
}
function rig() {
  const calls: string[] = [];
  const entries: EntryDraft[] = [];
  const sink: DurableStageSink = {
    versions: { durable: "1.5.0", durablePiAi: "1.5.0", codingAgent: "1.5.2", codingAgentPiAi: "1.5.2" },
    rootAbsent: async (id) => (calls.push(`absent:${id}`), true),
    createRoot: async ({ cpdSessionId, agent, init }) => {
      calls.push(`create:${cpdSessionId}`);
      assert.deepEqual(agent, snapshot().activeAgent);
      await init({ appendEntry: (async (conversationId: ConversationId, draft: EntryDraft) => {
        entries.push(draft);
        return { ...draft, head: undefined, id: entries.length as EntryId, conversationId };
      }) as PublicTx["appendEntry"] }, "durable-root");
      return { conversationId: "durable-root" };
    },
  };
  return { sink, calls, entries };
}
function contains(value: unknown, needle: unknown): boolean {
  if (JSON.stringify(value) === JSON.stringify(needle)) return true;
  return value !== null && typeof value === "object" && Object.values(value).some((v) => contains(v, needle));
}

test("staging preserves identity and provenance", async () => {
  const input = snapshot(); const before = structuredClone(input); const r = rig();
  const result = await stageNativeConversation(input, r.sink);
  assert.equal(result.status, "staged");
  if (result.status !== "staged") return;
  assert.equal(result.cpdSessionId, input.cpdSessionId);
  assert.equal(result.durableConversationId, "durable-root");
  assert.equal(result.sourceSha256, input.sourceSha256);
  assert.equal(result.stagedOnly, true);
  assert.equal(result.importVersion, "cpd-native-transcript/1");
  assert.equal(result.leafSourceId, "z");
  assert.deepEqual(result.activeProjectionSourceIds, ["u", "a", "t", "z"]);
  assert.deepEqual(result.sourceToDurableEntryId, Object.fromEntries(rows.map((e, i) => [e.id, String(i + 1)])));
  assert.deepEqual(r.calls, ["absent:cpd-session-1", "create:cpd-session-1"]);
  assert.deepEqual(r.entries.map((e) => e.kind), [...rows.map(() => "cpd.native-entry"), "cpd.native-header", ...["u", "a", "t", "z"].map(() => "cpd.native-projection"), "cpd.native-map"]);
  assert.deepEqual(r.entries[rows.length]?.model, []);
  for (const fact of [header, input.sourceBytesBase64, input.sourceSha256, input.tree, input.leafId]) assert.ok(contains(r.entries[rows.length]?.data, fact));
  rows.forEach((entry, index) => {
    const draft = r.entries[index]!;
    assert.deepEqual(draft.model, []);
    for (const fact of [entry.raw, entry.id, entry.parentId, entry.timestamp, entry.type, input.sourcePath, input.cpdSessionId, input.sourceSha256]) assert.ok(contains(draft.data, fact), `missing provenance ${entry.id}: ${JSON.stringify(fact)}`);
  });
  const projections = r.entries.filter((e) => e.kind === "cpd.native-projection");
  ["u", "a", "t", "z"].forEach((id, i) => assert.ok(contains(projections[i]?.data, id)));
  assert.ok(contains(projections.map((e) => e.model), "call-1"));
  assert.ok(contains(r.entries.at(-1)?.data, result.sourceToDurableEntryId));
  assert.deepEqual(input, before);
});

test("staging refuses unsupported state", async () => {
  const unknown = row("bad", "z", { type: "future_entry", payload: { state: true } });
  const base = snapshot();
  const brokenRows = rows.map((e) => e.id === "t" ? row("t", "a", { type: "message", message: { role: "toolResult", toolCallId: "missing", toolName: "read", content: [{ type: "text", text: "contents" }], isError: false, timestamp: 3 } }) : e);
  const cases: { input: NativeSnapshot; code: string | readonly string[]; entry?: string }[] = [
    { input: { ...snapshot([...rows, unknown]), leafId: "bad" }, code: "unsupported-entry", entry: "bad" },
    { input: { ...base, tree: base.tree.map((e) => e.id === "u" ? { ...e, parentId: "z" } : e) }, code: "invalid-tree" },
    { input: snapshot(brokenRows), code: ["invalid-projection", "invalid-message"] },
    { input: { ...base, sourceSha256: "0".repeat(64) }, code: "invalid-snapshot" },
  ];
  for (const { input, code, entry } of cases) {
    const r = rig(); const before = structuredClone(input);
    const result = await stageNativeConversation(input, r.sink);
    assert.equal(result.status, "refused");
    if (result.status !== "refused") continue;
    assert.ok((typeof code === "string" ? [code] : code).includes(result.code));
    if (entry) assert.equal(result.sourceEntryId, entry);
    assert.equal(result.cpdSessionId, input.cpdSessionId);
    assert.equal(result.sourceSha256, input.sourceSha256);
    assert.deepEqual(r.calls, []);
    assert.deepEqual(r.entries, []);
    assert.deepEqual(input, before);
  }
});
