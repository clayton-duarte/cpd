import { describe, expect, it } from "vitest";
import { createDialect } from "./b91-serving-backport.ts";

type Entry = Record<string, unknown>;
const user = (id: number, text: string): Entry => ({ id, conversationId: "root", kind: "pi.user", model: [{ role: "user", content: text }] });
const data = { skill: "cpd-lead", files: { "SKILL.md": "a".repeat(64) } };
const baseline = (): Entry => ({ id: 2, conversationId: "root", kind: "cpd-skills", model: [], data });
const notice = (model: unknown): Entry => ({ id: 3, conversationId: "root", kind: "cpd-skills", model, data });
function load(entries: Entry[], protocol: 1 | 2) {
  const dialect = createDialect(protocol);
  if (protocol === 1) return dialect.applyBase({ snapshot: { type: "snapshot", entries, tools: [], nestedTools: [], compactions: [], inbox: [], agent: {}, usage: { models: {}, tools: {} } } }).transcript.items;
  return dialect.applyBase({ view: { conversation: "root", entries, docs: { "pi.agent": {}, "pi.live": {}, "pi.inbox": { items: [] }, "pi.provider": {}, "pi.usage": { models: {}, tools: {} } } } }).transcript.items;
}
function errorCode(fn: () => unknown): unknown {
  try { fn(); return "no error"; } catch (error) { return (error as { code?: unknown }).code; }
}

describe("deployed skills backport", () => {
  it("renders deployed skills metadata", () => {
    for (const protocol of [1, 2] as const) {
      const metadata = baseline();
      const entries = [user(1, "before"), metadata, notice([{ role: "user", content: [{ type: "text", text: "Skills changed" }, { type: "text", text: "details" }] }]), user(4, "after")];
      const items = load(entries, protocol);
      expect(items.map((item) => item.id)).toEqual(["u:1", "f:3", "u:4"]);
      expect(items[1]).toMatchObject({ kind: "fold", foldKind: "cpd-skills", title: "Skills changed", body: "Skills changed\ndetails", defaultOpen: false });
      expect(metadata.data).toEqual(data);
      expect(load([user(1, "before"), { ...baseline(), data: { skill: "cpd-lead", files: {} } }, user(4, "after")], protocol).map((item) => item.id)).toEqual(["u:1", "u:4"]);
    }
  });

  it("rejects malformed deployed message", () => {
    for (const protocol of [1, 2] as const) {
      const malformed: Entry[] = [
        { ...baseline(), data: undefined }, { ...baseline(), data: { skill: 1, files: {} } },
        { ...baseline(), data: { skill: "x", files: { a: "A".repeat(64) } } },
        notice([{ role: "assistant", content: "wrong role" }]), notice([{ role: "user", content: [{ type: "image" }] }]),
        notice([{ role: "user", content: null }]), notice([null]),
        { id: 3, kind: "pi.user", model: [] }, { id: 3, kind: "pi.assistant", model: [null] },
        { id: 3, kind: "pi.tool-result", model: [{ role: "user", content: "bad" }] },
      ];
      for (const bad of malformed) expect(errorCode(() => load([user(1, "before"), bad, user(4, "after")], protocol)), JSON.stringify(bad)).toBe("protocol_error");
    }
  });
});
