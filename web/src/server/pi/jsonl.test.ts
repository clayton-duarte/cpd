import { describe, expect, it } from "vitest";
import { WebError } from "../../protocol/domain.ts";
import type { FoldItem, ToolItem } from "../../protocol/domain.ts";
import { countItems, createNormalizer, normalizeText, parseLine } from "./jsonl.ts";

const line = (o: unknown): string => JSON.stringify(o);
const userMsg = (id: string, content: unknown) =>
  line({ type: "message", id, timestamp: "2026-11-03T16:47:40.114Z", message: { role: "user", content } });
const assistantMsg = (id: string, content: unknown[], extra: Record<string, unknown> = {}) =>
  line({ type: "message", id, message: { role: "assistant", content, model: "vertex/claude", stopReason: "stop", ...extra } });
const toolResult = (id: string, callId: string, toolName: string, text: string, extra: Record<string, unknown> = {}) =>
  line({
    type: "message",
    id,
    message: { role: "toolResult", toolCallId: callId, toolName, content: [{ type: "text", text }], isError: false, durationMs: 5, ...extra },
  });
const custom = (id: string, customType: string, content: string, details: unknown, display: boolean = true) =>
  line({ type: "custom_message", id, customType, content, display, details });

describe("pi jsonl normalisation", () => {
  it("message lines normalise to user and assistant items", () => {
    const text = [
      line({ type: "session", id: "sess", cwd: "/w" }),
      line({ type: "message", id: "s0", message: { role: "system", content: "" } }),
      userMsg("u1", [{ type: "text", text: "hello" }, { type: "text", text: "world" }]),
      assistantMsg("a1", [
        { type: "thinking", thinking: "hmm", thinkingSignature: "sig" },
        { type: "text", text: "hi" },
      ]),
      "",
    ].join("\n");
    const { items, facts } = normalizeText(text);
    expect(items.map((i) => i.id)).toEqual(["u:u1", "a:a1"]);
    expect(items[0]).toMatchObject({ kind: "user", text: "hello\nworld", attachments: [] });
    expect(items[1]).toMatchObject({
      kind: "assistant",
      blocks: [
        { type: "thinking", text: "hmm" },
        { type: "text", text: "hi" },
      ],
      streaming: false,
      model: "vertex/claude",
    });
    expect(facts.cwd).toBe("/w");
    expect(facts.lastRole).toBe("assistant");
  });

  it("tool calls pair with tool results across lines", () => {
    const n = createNormalizer();
    const first = n.push(assistantMsg("a1", [{ type: "toolCall", id: "c1", name: "read", arguments: { path: "x" } }], { stopReason: "toolUse" }), { offset: 0 });
    expect(first.added).toHaveLength(1);
    expect(first.added[0]).toMatchObject({ kind: "tool", id: "tool:c1", callId: "c1", name: "read", args: { path: "x" }, status: "running" });
    const second = n.push(toolResult("r1", "c1", "read", "contents"), { offset: 100 });
    expect(second.added).toEqual([]);
    expect(second.updated).toHaveLength(1);
    expect(second.updated[0]).toMatchObject({
      kind: "tool",
      id: "tool:c1",
      status: "done",
      result: { text: "contents", isError: false, durationMs: 5 },
    });
    const failed = createNormalizer();
    failed.push(assistantMsg("a2", [{ type: "toolCall", id: "c2", name: "bash", arguments: {} }]), { offset: 0 });
    const f = failed.push(toolResult("r2", "c2", "bash", "boom", { isError: true }), { offset: 10 });
    expect(f.updated[0]).toMatchObject({ status: "failed", result: { isError: true } });
    const whole = normalizeText([assistantMsg("a1", [{ type: "toolCall", id: "c1", name: "read", arguments: {} }]), toolResult("r1", "c1", "read", "ok")].join("\n"));
    expect(whole.items).toHaveLength(1);
    expect(whole.items[0]).toMatchObject({ kind: "tool", status: "done" });
  });

  it("a tool result without its call in the window becomes an orphan result item", () => {
    const n = createNormalizer();
    const r = n.push(toolResult("r9", "gone", "read", "late"), { offset: 0 });
    expect(r.updated).toEqual([]);
    expect(r.added).toEqual([
      { kind: "orphanResult", id: "orphan:gone", callId: "gone", name: "read", result: { text: "late", isError: false, durationMs: 5 } },
    ]);
  });

  it("compaction entries become compaction items", () => {
    const { items } = normalizeText(line({ type: "compaction", id: "k1", timestamp: "t", summary: "earlier work", firstKeptEntryId: "x", tokensBefore: 1234 }));
    expect(items).toEqual([expect.objectContaining({ kind: "compaction", id: "c:k1", summary: "earlier work", tokensBefore: 1234 })]);
    expect(() => normalizeText(line({ type: "compaction", id: "k2" }))).toThrowError(WebError);
  });

  it("model_change and thinking_level_change update the footer state", () => {
    const n = createNormalizer();
    const m = n.push(line({ type: "model_change", id: "m1", provider: "bifrost", modelId: "vertex/claude-sonnet-5-5" }), { offset: 0 });
    expect(m.factsChanged).toBe(true);
    expect(n.facts.model).toBe("bifrost/vertex/claude-sonnet-5-5");
    const t = n.push(line({ type: "thinking_level_change", id: "t1", thinkingLevel: "high" }), { offset: 50 });
    expect(t.factsChanged).toBe(true);
    expect(n.facts.thinking).toBe("high");
    expect(m.added).toEqual([]);
    expect(t.added).toEqual([]);
    expect(normalizeText(line({ type: "session_info", id: "i", name: "r-orc" })).facts.name).toBe("r-orc");
  });

  it("cpd custom messages fold with the right default open state", () => {
    const text = [
      custom("f1", "cpd-context", "Context body\nsecond", { summary: "6 records" }),
      custom("f2", "cpd-plan-summary", "The summary", { review: "r3" }),
      custom("f3", "cpd-plan-decision", "ok", { review: "r3", approved: true }),
      custom("f4", "cpd-plan-decision", "no", { review: "r4", approved: false }),
      custom("f5", "cpd-skills", "Skills line one\nmore", {}),
      custom("f6", "cpd-context", "First line fallback\nrest", {}),
    ].join("\n");
    const folds = normalizeText(text).items as FoldItem[];
    expect(folds.map((f) => [f.id, f.foldKind, f.title, f.defaultOpen])).toEqual([
      ["f:f1", "cpd-context", "6 records", false],
      ["f:f2", "cpd-plan-summary", "Plan review r3 summary", true],
      ["f:f3", "cpd-plan-decision", "Plan review r3 approved", false],
      ["f:f4", "cpd-plan-decision", "Plan review r4 rejected", false],
      ["f:f5", "cpd-skills", "Skills line one", false],
      ["f:f6", "cpd-context", "First line fallback", false],
    ]);
    expect(folds[1]?.body).toBe("The summary");
  });

  it("hidden cpd custom messages are dropped", () => {
    const text = [
      custom("h1", "cpd-first-message", "x", {}, false),
      custom("h2", "cpd-reminder", "x", {}),
      custom("h3", "cpd-context", "hidden by display", {}, false),
      custom("h5", "something-else", "x", {}),
    ].join("\n");
    expect(normalizeText(text).items).toEqual([]);
  });

  const cpdMsg = (id: string, content: string, details: unknown, display: boolean = true) =>
    line({ type: "custom_message", id, customType: "cpd", content, display, details, timestamp: "2026-11-03T20:28:42.748Z" });
  const fromNull = cpdMsg(
    "d0c4d136",
    "Message from the User (15:28 EST): Drafts of Plan x are parked",
    { id: 96920, at: "2026-11-03T20:28:42.459Z", kind: "session-message", text: "Message from the User: Drafts", data: { from: null, text: "Drafts of Plan x are parked", wake: true } },
  );
  const fromSession = cpdMsg(
    "e1",
    "Message from cpd-context (75b61f97) (16:03 EST): From the Manager hello",
    { kind: "session-message", text: "Message from cpd-context: From the Manager hello", data: { from: "75b61f97-8377-4feb-97e6-2f1c1d1ed439", text: "From the Manager hello" } },
  );
  const engine = cpdMsg("e2", "Job scout-merge (scout) merged: findings in x.md.\nSummary: s", {
    kind: "job-merged",
    text: "Job scout-merge (scout) merged: findings in x.md.\nSummary: s",
    data: { artefact: "x.md" },
  });

  it("cpd session-message from null becomes a user item", () => {
    const { items } = normalizeText(fromNull);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      kind: "user",
      id: "u:d0c4d136",
      text: "Drafts of Plan x are parked",
      at: "2026-11-03T20:28:42.459Z",
      attachments: [],
    });
  });

  it("cpd session-message from a session becomes a cpd fold titled by sender", () => {
    const { items } = normalizeText(fromSession);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      kind: "fold",
      id: "f:e1",
      foldKind: "cpd",
      defaultOpen: false,
      title: "Message from 75b61f97-8377-4feb-97e6-2f1c1d1ed439",
      body: "Message from cpd-context (75b61f97) (16:03 EST): From the Manager hello",
    });
  });

  it("cpd engine event becomes a collapsed cpd fold", () => {
    const { items } = normalizeText(engine);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      kind: "fold",
      id: "f:e2",
      foldKind: "cpd",
      defaultOpen: false,
      title: "Job scout-merge (scout) merged: findings in x.md.",
    });
  });

  it("cpd custom messages count as one item each for the window", () => {
    const counts = [fromNull, fromSession, engine].map((l, i) => countItems(parseLine(l, { offset: i })));
    expect(counts).toEqual([1, 1, 1]);
  });

  it("cpd custom message with display false is dropped", () => {
    const hidden = cpdMsg("x1", "hidden", { kind: "job-merged", text: "hidden", data: {} }, false);
    expect(normalizeText([hidden, engine].join("\n")).items.map((i) => i.id)).toEqual(["f:e2"]);
    expect(countItems(parseLine(hidden, { offset: 0 }))).toBe(0);
  });

  it("cpd session-message from null sets lastRole user", () => {
    const text = [assistantMsg("a1", [{ type: "text", text: "hi" }]), fromNull].join("\n");
    expect(normalizeText(text).facts.lastRole).toBe("user");
  });

  it("malformed lines raise a typed error with the line number", () => {
    const caught = (fn: () => unknown): WebError => {
      try {
        fn();
      } catch (e) {
        if (e instanceof WebError) return e;
        throw e;
      }
      throw new Error("did not throw");
    };
    const whole = caught(() => normalizeText([userMsg("u1", "ok"), "{not json", userMsg("u2", "ok")].join("\n")));
    expect(whole.code).toBe("malformed_line");
    expect(whole.details).toMatchObject({ line: 2 });
    const noType = caught(() => parseLine("{}", { offset: 77 }));
    expect(noType.code).toBe("malformed_line");
    expect(noType.details).toMatchObject({ offset: 77 });
    expect(noType.details).not.toHaveProperty("line");
    expect(caught(() => parseLine("[1]", { line: 3, offset: 0 })).details).toMatchObject({ line: 3, offset: 0 });
    expect(caught(() => normalizeText(line({ type: "message", id: "x" }))).code).toBe("malformed_line");
    expect(parseLine(line({ type: "label", id: "z" }), { offset: 0 })).toMatchObject({ type: "label", id: "z" });
  });

  it("edit results carry a diff", () => {
    const text = [
      assistantMsg("a1", [
        { type: "toolCall", id: "e1", name: "edit", arguments: { path: "f" } },
        { type: "toolCall", id: "e2", name: "edit", arguments: { path: "g" } },
        { type: "toolCall", id: "b1", name: "bash", arguments: {} },
      ]),
      toolResult("r1", "e1", "edit", "edited", { details: { patch: "--- a\n+++ b\n+x", diff: "ignored" } }),
      toolResult("r2", "e2", "edit", "edited", { details: { diff: "-a\n+b" } }),
      toolResult("r3", "b1", "bash", "out", { details: { diff: "not for bash" } }),
    ].join("\n");
    const tools = normalizeText(text).items as ToolItem[];
    expect(tools.map((t) => t.diff)).toEqual(["--- a\n+++ b\n+x", "-a\n+b", undefined]);
  });

  it("assistant errorMessage renders as an Error text block and is counted", () => {
    const errorMessage = "Error reading stream: stream idle timeout: no data received within configured window";
    const expected = [{ type: "text", text: "Error: "+"Error reading stream: stream idle timeout: no data received within configured window" }];
    for (const [i, content] of [[{ type: "thinking", thinking: "", thinkingSignature: "sig" }], []].entries()) {
      const l = assistantMsg("e" + i, content, { stopReason: "error", errorMessage });
      const { added } = createNormalizer().push(l, { offset: 0 });
      expect(added).toHaveLength(1);
      expect(added[0]).toMatchObject({ kind: "assistant", blocks: expected });
      expect(countItems(parseLine(l, { offset: 0 }))).toBe(1);
    }
  });
});
