import { describe, expect, it } from "vitest";
import {
  deriveTitle,
  shapeConversationTree,
  openTitleStore,
  resolveConversationId,
  resolvePromptConversationId,
  addJob,
  removeJob,
  setStatus,
  validateGraph,
  DuplicateJobIdError,
  UnknownJobIdError,
  type Job,
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

  it("does not fall back to root for an explicit non-numeric value", () => {
    const url = new URL("http://localhost/api/messages?conversation=bogus");
    expect(Number.isNaN(resolveConversationId(url, 1))).toBe(true);
  });
});

describe("resolvePromptConversationId", () => {
  it("resolves to the root id when neither query nor body supply a conversation", () => {
    const url = new URL("http://localhost/api/prompt");
    expect(resolvePromptConversationId(url, undefined, 1)).toEqual({ kind: "ok", id: 1 });
  });

  it("resolves to the query id when only the query param is present", () => {
    const url = new URL("http://localhost/api/prompt?conversation=16");
    expect(resolvePromptConversationId(url, undefined, 1)).toEqual({ kind: "ok", id: 16 });
  });

  it("resolves to the body id when only the body supplies one", () => {
    const url = new URL("http://localhost/api/prompt");
    expect(resolvePromptConversationId(url, 16, 1)).toEqual({ kind: "ok", id: 16 });
  });

  it("resolves when query and body agree", () => {
    const url = new URL("http://localhost/api/prompt?conversation=16");
    expect(resolvePromptConversationId(url, 16, 1)).toEqual({ kind: "ok", id: 16 });
  });

  it("reports a conflict when query and body disagree", () => {
    const url = new URL("http://localhost/api/prompt?conversation=16");
    expect(resolvePromptConversationId(url, 17, 1)).toEqual({ kind: "conflict" });
  });
});

describe("addJob", () => {
  it("appends a job", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
    expect(addJob([], j1)).toEqual([j1]);
  });

  it("rejects a duplicate id", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
    const dupe: Job = { id: "j1", title: "Other", status: "draft", needs: [] };
    expect(() => addJob([j1], dupe)).toThrow(DuplicateJobIdError);
  });
});

describe("removeJob", () => {
  it("removes the job and strips it from every other job's needs, leaving no dangling edges", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
    const j2: Job = { id: "j2", title: "Test", status: "draft", needs: ["j1"] };
    const j3: Job = { id: "j3", title: "Deploy", status: "draft", needs: ["j1", "j2"] };

    const result = removeJob([j1, j2, j3], "j1");

    expect(result).toEqual([
      { id: "j2", title: "Test", status: "draft", needs: [] },
      { id: "j3", title: "Deploy", status: "draft", needs: ["j2"] },
    ]);
  });
});

describe("setStatus", () => {
  it("updates the status of the matching job only", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
    const j2: Job = { id: "j2", title: "Test", status: "draft", needs: [] };

    const result = setStatus([j1, j2], "j1", "queued");

    expect(result).toEqual([
      { id: "j1", title: "Build", status: "queued", needs: [] },
      { id: "j2", title: "Test", status: "draft", needs: [] },
    ]);
  });

  it("throws for an unknown id", () => {
    expect(() => setStatus([], "missing", "queued")).toThrow(UnknownJobIdError);
  });
});

describe("validateGraph", () => {
  it("passes a valid DAG", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: [] };
    const j2: Job = { id: "j2", title: "Test", status: "draft", needs: ["j1"] };
    expect(validateGraph([j1, j2])).toBeUndefined();
  });

  it("detects an unknown id referenced by needs", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: ["ghost"] };
    expect(validateGraph([j1])).toEqual({ kind: "dangling", jobId: "j1", missingId: "ghost" });
  });

  it("detects a self-edge as a cycle", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: ["j1"] };
    const result = validateGraph([j1]);
    expect(result).toEqual({ kind: "cycle", ids: ["j1"] });
  });

  it("detects a 2-cycle", () => {
    const j1: Job = { id: "j1", title: "Build", status: "draft", needs: ["j2"] };
    const j2: Job = { id: "j2", title: "Test", status: "draft", needs: ["j1"] };
    const result = validateGraph([j1, j2]);
    expect(result?.kind).toBe("cycle");
    expect((result as { ids: string[] }).ids.sort()).toEqual(["j1", "j2"]);
  });

  it("detects a 3-cycle", () => {
    const j1: Job = { id: "j1", title: "A", status: "draft", needs: ["j2"] };
    const j2: Job = { id: "j2", title: "B", status: "draft", needs: ["j3"] };
    const j3: Job = { id: "j3", title: "C", status: "draft", needs: ["j1"] };
    const result = validateGraph([j1, j2, j3]);
    expect(result?.kind).toBe("cycle");
    expect((result as { ids: string[] }).ids.sort()).toEqual(["j1", "j2", "j3"]);
  });
});
