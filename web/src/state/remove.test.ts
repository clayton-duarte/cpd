import { describe, expect, it } from "vitest";
import type { AssistantItem, Capabilities, FooterStats, SessionRef, ToolItem } from "../protocol/domain.ts";
import type { ServerMessage } from "../protocol/wire.ts";
import { initialState, reduce, type AppState, type StoreAction } from "./store.ts";
import { appendItems, emptyTranscript, removeItems } from "./transcript.ts";

const assistant = (id: string, text: string): AssistantItem => ({ kind: "assistant", id, blocks: [{ type: "text", text }], streaming: true });
const tool = (callId: string): ToolItem => ({ kind: "tool", id: `tool:${callId}`, callId, name: "read", args: {}, status: "running" });
const seeded = () => appendItems(emptyTranscript, [assistant("live:1", "a"), tool("c1"), assistant("a:e2", "b")]);

describe("remove items", () => {
  it("remove items drops the item with that id", () => {
    const next = removeItems(seeded(), ["live:1"]);
    expect(next.items.map((i) => i.id)).toEqual(["tool:c1", "a:e2"]);
  });

  it("remove items ignores an unknown id", () => {
    const state = seeded();
    expect(removeItems(state, ["live:999"])).toBe(state);
  });
});

const unknown = { state: "unknown", reason: "n/a" } as const;
const footer: FooterStats = { model: unknown, tier: unknown, thinking: unknown, contextPercent: unknown, cost: unknown, branch: unknown };
const ref = { id: "s1", key: "c:p:j", runner: "pi", role: "worker", state: "working", title: "T" } as SessionRef;
const run = (state: AppState, ...actions: StoreAction[]): AppState => actions.reduce(reduce, state);
const server = (message: ServerMessage): StoreAction => ({ type: "server", message });

describe("the store reducer", () => {
  it("the store reducer applies session remove", () => {
    const before = run(
      initialState,
      { type: "selectSession", sessionId: "s1" },
      server({ type: "session.attached", sessionId: "s1", capabilities: {} as Capabilities, title: "T", meta: { ref, footer } }),
      server({ type: "session.append", sessionId: "s1", items: [assistant("live:1", "a"), tool("c1")] }),
    );
    expect(before.session?.transcript.items).toHaveLength(2);
    const after = run(before, server({ type: "session.remove", sessionId: "s1", itemId: "live:1" }));
    expect(after.session?.transcript.items.map((i) => i.id)).toEqual(["tool:c1"]);
  });
});
