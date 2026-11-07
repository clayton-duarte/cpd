import { describe, expect, it } from "vitest";
import type { AssistantItem, ToolItem, TranscriptItem, UserItem, Window } from "../protocol/domain.ts";
import * as transcript from "./transcript.ts";
import { appendItems, applyDelta, emptyTranscript, prependEarlier, setTail, updateItems } from "./transcript.ts";

// 15.1: a pure helper turns a failed tool's result into the toast's reason. The export arrives with the
// fix; the cast keeps this red test compiling until then.
type ToolFailureReason = (tool: ToolItem) => string;
const reasonOf =
  (transcript as unknown as { toolFailureReason?: ToolFailureReason }).toolFailureReason ?? (() => "");

const user = (id: string, text = id): UserItem => ({ kind: "user", id, text, attachments: [] });
const assistant = (id: string, text: string): AssistantItem => ({
  kind: "assistant", id, blocks: [{ type: "text", text }], streaming: true,
});
const tool = (callId: string, over: Partial<ToolItem> = {}): ToolItem => ({
  kind: "tool", id: `tool:${callId}`, callId, name: "bash", args: {}, status: "running", ...over,
});
const win = (items: TranscriptItem[], cursor: string | null): Window => ({
  items, hasEarlier: cursor !== null, cursor,
});
const ids = (items: readonly TranscriptItem[]): string[] => items.map((i) => i.id);

describe("transcript", () => {
  it("tail window sets items and hasEarlier", () => {
    const old = setTail(win([user("u:old")], null));
    const s = setTail(win([user("u:1"), user("u:2")], "pi:10"));
    expect(ids(s.items)).toEqual(["u:1", "u:2"]);
    expect(s.hasEarlier).toBe(true);
    expect(s.cursor).toBe("pi:10");
    const replaced = setTail(win([user("u:3")], null));
    expect(ids(replaced.items)).toEqual(["u:3"]);
    expect(replaced.hasEarlier).toBe(false);
    expect(replaced.cursor).toBeNull();
    expect(ids(old.items)).toEqual(["u:old"]);
    expect(emptyTranscript).toEqual({ items: [], hasEarlier: false, cursor: null, tailEpoch: 0 });
  });

  it("loadEarlier prepends older items without duplicates", () => {
    const base = setTail(win([user("u:3"), user("u:4")], "pi:30"));
    const frozen = JSON.stringify(base);
    const s = prependEarlier(base, win([user("u:1"), user("u:2"), user("u:3")], "pi:5"));
    expect(ids(s.items)).toEqual(["u:1", "u:2", "u:3", "u:4"]);
    expect(s.cursor).toBe("pi:5");
    expect(s.hasEarlier).toBe(true);
    expect(JSON.stringify(base)).toBe(frozen);
    const last = prependEarlier(s, win([user("u:0")], null));
    expect(ids(last.items)).toEqual(["u:0", "u:1", "u:2", "u:3", "u:4"]);
    expect(last.hasEarlier).toBe(false);
    expect(last.cursor).toBeNull();
  });

  it("append and update items merge by id", () => {
    const base = setTail(win([user("u:1", "a"), user("u:2", "b")], null));
    const appended = appendItems(base, [user("u:3", "c"), user("u:1", "A")]);
    expect(ids(appended.items)).toEqual(["u:1", "u:2", "u:3"]);
    expect((appended.items[0] as UserItem).text).toBe("A");
    const updated = updateItems(appended, [user("u:2", "B"), user("u:99", "ghost")]);
    expect(ids(updated.items)).toEqual(["u:1", "u:2", "u:3"]);
    expect((updated.items[1] as UserItem).text).toBe("B");
    expect(ids(base.items)).toEqual(["u:1", "u:2"]);
  });

  it("streaming delta appends to the matching block", () => {
    const base = setTail(win([user("u:1"), assistant("a:1", "Hel")], null));
    const more = applyDelta(base, { itemId: "a:1", blockIndex: 0, blockType: "text", text: "lo" });
    expect((more.items[1] as AssistantItem).blocks).toEqual([{ type: "text", text: "Hello" }]);
    const added = applyDelta(more, { itemId: "a:1", blockIndex: 1, blockType: "thinking", text: "hmm" });
    expect((added.items[1] as AssistantItem).blocks).toEqual([
      { type: "text", text: "Hello" },
      { type: "thinking", text: "hmm" },
    ]);
    expect(() => applyDelta(added, { itemId: "a:1", blockIndex: 5, blockType: "text", text: "x" })).toThrow(
      expect.objectContaining({ code: "protocol_error" }),
    );
    expect(() => applyDelta(added, { itemId: "u:1", blockIndex: 0, blockType: "text", text: "x" })).toThrow(
      expect.objectContaining({ code: "protocol_error" }),
    );
    expect(applyDelta(added, { itemId: "nope", blockIndex: 0, blockType: "text", text: "x" })).toBe(added);
  });

  it("a failed tool reason is its first non-empty line", () => {
    const leading = tool("c1", { status: "failed", result: { text: "\n\n  boom  \nmore", isError: true } });
    expect(reasonOf(leading)).toBe("boom");
    const first = tool("c2", { status: "failed", result: { text: "first\nsecond", isError: true } });
    expect(reasonOf(first)).toBe("first");
  });

  it("tool result updates its tool item", () => {
    const base = setTail(win([user("u:1"), tool("c1"), user("u:2")], null));
    const done = tool("c1", { status: "done", result: { text: "ok", isError: false } });
    const s = updateItems(base, [done]);
    expect(ids(s.items)).toEqual(["u:1", "tool:c1", "u:2"]);
    expect(s.items[1]).toEqual(done);
    const orphan: TranscriptItem = {
      kind: "orphanResult", id: "orphan:c2", callId: "c2", name: "bash", result: { text: "late", isError: true },
    };
    const withOrphan = setTail(win([orphan], null));
    const adopted = appendItems(withOrphan, [tool("c2")]);
    expect(ids(adopted.items)).toEqual(["tool:c2"]);
    expect((adopted.items[0] as ToolItem).status).toBe("failed");
    expect((adopted.items[0] as ToolItem).result?.text).toBe("late");
  });

  it("setTail increments tailEpoch", () => {
    const first = setTail(win([user("u:1")], null));
    const second = setTail(win([user("u:2")], null), first);
    const third = setTail(win([user("u:3")], null), second);
    expect(first.tailEpoch).toBe(1);
    expect(second.tailEpoch).toBe(2);
    expect(third.tailEpoch).toBe(3);
  });

  it("prependEarlier keeps tailEpoch", () => {
    const base = setTail(win([user("u:3")], "pi:3"), setTail(win([], null)));
    expect(base.tailEpoch).toBe(2);
    expect(prependEarlier(base, win([user("u:2")], null)).tailEpoch).toBe(2);
  });

  it("applyDelta appends text without copying other items", () => {
    // The streaming item is last: the search runs from the END, so an earlier item with the same id is untouched.
    const first = assistant("a:1", "old");
    const other = user("u:1");
    const last = assistant("a:1", "Hel");
    const base = setTail(win([first, other, last], null));
    const s = applyDelta(base, { itemId: "a:1", blockIndex: 0, blockType: "text", text: "lo" });
    expect(s.items[0]).toBe(first);
    expect(s.items[1]).toBe(other);
    expect((s.items[2] as AssistantItem).blocks).toEqual([{ type: "text", text: "Hello" }]);
    expect(s.tailEpoch).toBe(base.tailEpoch);
  });

  it("appendItems of several items keeps order and replaces by id", () => {
    const base = setTail(win([user("u:1", "a"), user("u:2", "b")], null), setTail(win([], null)));
    const s = appendItems(base, [user("u:3", "c"), user("u:2", "B"), user("u:4", "d"), user("u:1", "A")]);
    expect(ids(s.items)).toEqual(["u:1", "u:2", "u:3", "u:4"]);
    expect(s.items.map((i) => (i as UserItem).text)).toEqual(["A", "B", "c", "d"]);
    expect(s.tailEpoch).toBe(2);
  });
});
