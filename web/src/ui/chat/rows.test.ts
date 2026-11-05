import { expect, it } from "vitest";
import type { AssistantItem, ToolItem, TranscriptItem, UserItem } from "../../protocol/domain.ts";
import { groupRows as groupRowsTyped } from "./rows.ts";

// The `chain` row kind is added by the implementation (DESIGN-chat 2.4.2): read the rows through a loose shape.
interface LooseStep { kind: string; key: string; tool?: ToolItem; tools?: readonly ToolItem[] }
interface LooseRow {
  kind: string; key: string; steps: readonly LooseStep[];
  stepCount: number; toolCount: number; thoughtCount: number; failedCount: number; running: boolean;
}
const groupRows = (items: readonly TranscriptItem[]): LooseRow[] => groupRowsTyped(items) as unknown as LooseRow[];

const tool = (id: string): ToolItem => ({ kind: "tool", id: `tool:${id}`, callId: id, name: "read", args: {}, status: "running" });
const user = (id: string): TranscriptItem => ({ kind: "user", id: `u:${id}`, text: id, attachments: [] }) as UserItem;
const asst = (id: string): TranscriptItem => ({ kind: "assistant", id: `a:${id}`, blocks: [{ type: "text", text: id }] }) as unknown as AssistantItem;
const thought = (id: string): TranscriptItem => ({ kind: "assistant", id: `a:${id}`, blocks: [{ type: "thinking", text: id }] }) as unknown as AssistantItem;

it("groupRows turns a run of tool items into one chain row of a tools step", () => {
  const t1 = tool("1");
  const t2 = tool("2");
  const rows = groupRows([user("x"), t1, t2, asst("y")]);
  expect(rows.map((r) => r.kind)).toEqual(["item", "chain", "item"]);
  const chain = rows[1]!;
  expect(chain.stepCount).toBe(2);
  expect(chain.toolCount).toBe(2);
  expect(chain.steps.map((s) => s.kind)).toEqual(["tools"]);
  expect(chain.steps[0]!.tools).toEqual([t1, t2]);
});

it("groupRows puts a lone tool item in a chain row of one tool step", () => {
  const t1 = tool("1");
  const rows = groupRows([t1, user("x"), tool("2")]);
  expect(rows.map((r) => r.kind)).toEqual(["chain", "item", "chain"]);
  const chain = rows[0]!;
  expect(chain.stepCount).toBe(1);
  expect(chain.steps).toEqual([{ kind: "tool", key: "tool:1", tool: t1 }]);
});

it("groupRows groups consecutive thought and tool items into one chain that text ends", () => {
  const rows = groupRows([user("x"), thought("p"), tool("1"), tool("2"), thought("q"), asst("answer"), tool("3")]);
  expect(rows.map((r) => r.kind)).toEqual(["item", "chain", "item", "chain"]);
  const chain = rows[1]!;
  expect(chain.steps.map((s) => s.kind)).toEqual(["thinking", "tools", "thinking"]);
  expect(chain.thoughtCount).toBe(2);
  expect(chain.toolCount).toBe(2);
  expect(chain.stepCount).toBe(4);
  expect(chain.key).toBe(`chain:${chain.steps[0]!.key}`);
});

it("groupRows counts failed tools and flags a running step on the chain", () => {
  const failed: ToolItem = { ...tool("1"), status: "failed" };
  const rows = groupRows([failed, tool("2")]);
  const chain = rows[0]!;
  expect(chain.failedCount).toBe(1);
  expect(chain.running).toBe(true);
});
