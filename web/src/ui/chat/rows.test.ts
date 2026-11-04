import { expect, it } from "vitest";
import type { AssistantItem, ToolItem, TranscriptItem, UserItem } from "../../protocol/domain.ts";
import { groupRows } from "./rows.ts";

const tool = (id: string): ToolItem => ({ kind: "tool", id: `tool:${id}`, callId: id, name: "read", args: {}, status: "running" });
const user = (id: string): TranscriptItem => ({ kind: "user", id: `u:${id}`, text: id, attachments: [] }) as UserItem;
const asst = (id: string): TranscriptItem => ({ kind: "assistant", id: `a:${id}`, blocks: [] }) as unknown as AssistantItem;

it("groupRows turns a run of two or more tool items into one tools row", () => {
  const t1 = tool("1");
  const t2 = tool("2");
  const rows = groupRows([user("x"), t1, t2, asst("y")]);
  expect(rows.map((r) => r.key)).toEqual(["u:x", "group:tool:1", "a:y"]);
  const g = rows[1]!;
  expect(g.kind).toBe("tools");
  if (g.kind === "tools") expect(g.tools).toEqual([t1, t2]);
});

it("groupRows keeps a lone tool item as an item row", () => {
  const t1 = tool("1");
  const rows = groupRows([t1, user("x"), tool("2")]);
  expect(rows.map((r) => [r.kind, r.key])).toEqual([["item", "tool:1"], ["item", "u:x"], ["item", "tool:2"]]);
  expect(rows[0]!.kind === "item" && rows[0]!.item).toBe(t1);
});

it("groupRows keeps row keys stable when a run grows", () => {
  const two = groupRows([user("x"), tool("1"), tool("2")]);
  const three = groupRows([user("x"), tool("1"), tool("2"), tool("3")]);
  expect(two.map((r) => r.key)).toEqual(["u:x", "group:tool:1"]);
  expect(three.map((r) => r.key)).toEqual(two.map((r) => r.key));
  const lone = groupRows([tool("1")]);
  expect(groupRows([tool("1"), tool("2")])[0]!.key).not.toBe(lone[0]!.key);
});
