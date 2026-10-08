import { describe, expect, it } from "vitest";
import {
  deriveTitle,
  shapeConversationTree,
  openTitleStore,
  resolveConversationId,
  type RawConversationRecord,
  type SqliteLike,
} from "./plans.ts";

describe("deriveTitle", () => {
  it("passes a short message through unchanged", () => {
    expect(deriveTitle("hello there")).toBe("hello there");
  });

  it("truncates a long message at a word boundary with an ellipsis", () => {
    const long =
      "this message is extremely long and definitely exceeds the sixty character budget for a title";
    const title = deriveTitle(long);
    expect(title.length).toBeLessThanOrEqual(61); // 60 + ellipsis char
    expect(title.endsWith("…")).toBe(true);
    expect(long.startsWith(title.slice(0, -1))).toBe(true);
    expect(title.slice(0, -1).endsWith(" ")).toBe(false);
  });

  it("falls back to Untitled for an empty or missing first message", () => {
    expect(deriveTitle("")).toBe("Untitled");
    expect(deriveTitle(undefined)).toBe("Untitled");
  });

  it("the root conversation is always titled Lead", () => {
    expect(deriveTitle("anything at all", { isRoot: true })).toBe("Lead");
  });
});

describe("shapeConversationTree", () => {
  it("maps raw scanConversations items into ConversationNode shape", () => {
    const raw: RawConversationRecord[] = [
      { id: 1 },
      { id: 12, parent: { conversationId: 1, at: 7 } },
    ];

    const shaped = shapeConversationTree(raw, () => "t");

    expect(shaped).toEqual([
      { id: 1, parentId: null, at: null, title: "t" },
      { id: 12, parentId: 1, at: 7, title: "t" },
    ]);
  });
});

/**
 * In-memory stand-in for pi-durable's `SqliteExecutor` (dist/storage/sqlite/database.d.ts),
 * which `openTitleStore` depends on instead of `node:sqlite` directly, so this test can run
 * through Vite's transform pipeline (which cannot load `node:sqlite` as an ES module import; see
 * the real `NodeSqliteDatabase` adapter in pi-durable, used unmodified from `index.ts`).
 * Exercises the real SQL text against real semantics (last-write-wins upsert).
 */
function fakeDatabase(initial = new Map<number, string>()): SqliteLike {
  const rows = initial;
  return {
    async exec() {
      // CREATE TABLE IF NOT EXISTS: no-op, the Map already exists.
    },
    async get<T extends object>(sql: string, ...params: (string | number)[]): Promise<T | undefined> {
      const id = params[0] as number;
      const title = rows.get(id);
      return title === undefined ? undefined : ({ title } as unknown as T);
    },
    async run(_sql: string, ...params: (string | number)[]): Promise<void> {
      const [id, title] = params as [number, string];
      rows.set(id, title);
    },
  };
}

describe("title override table", () => {
  it("a stored title wins over the derived one and round-trips", async () => {
    const backing = new Map<number, string>();
    const store = await openTitleStore(fakeDatabase(backing));

    expect(await store.get(1)).toBeUndefined();

    await store.set(1, "Custom title");
    expect(await store.get(1)).toBe("Custom title");

    await store.set(1, "Updated title");
    expect(await store.get(1)).toBe("Updated title");

    // "Restart": reopen a store over the same backing rows.
    const reopened = await openTitleStore(fakeDatabase(backing));
    expect(await reopened.get(1)).toBe("Updated title");
  });
});

describe("resolveConversationId", () => {
  it("resolves to the root id when no conversation param is present", () => {
    const url = new URL("http://localhost/api/messages");
    expect(resolveConversationId(url, 1)).toBe(1);
  });

  it("resolves to the parsed conversation id when present", () => {
    const url = new URL("http://localhost/api/messages?conversation=12");
    expect(resolveConversationId(url, 1)).toBe(12);
  });
});
