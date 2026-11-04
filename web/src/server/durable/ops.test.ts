import { describe, expect, it } from "vitest";
import { applyOps, createReplica } from "./ops.ts";

function deepFreeze<T>(v: T): T {
  if (v !== null && typeof v === "object") {
    Object.values(v as object).forEach(deepFreeze);
    Object.freeze(v);
  }
  return v;
}
const protocolError = { name: "WebError", code: "protocol_error" };
const LIVE = ["docs", "pi.live", "generation", "message", "content", 0, "text"];

function base(): Record<string, unknown> {
  return {
    entries: [{ id: 1 }, { id: 2 }],
    docs: { "pi.live": { generation: { message: { content: [{ type: "text", text: "he" }] } } }, other: { k: 1 } },
    list: [1, 2, 3],
  };
}

describe("applyOps", () => {
  it("applyOps replaces the root with an r op", () => {
    const root = deepFreeze(base());
    expect(applyOps(root, [["r", { fresh: true }]])).toEqual({ fresh: true });
    expect(applyOps(root, [["s", ["list", 0], 9], ["r", [1]]])).toEqual([1]);
  });

  it("applyOps sets an object key and an array index with s", () => {
    const out = applyOps(deepFreeze(base()), [["s", ["docs", "pi.live", "run"], { taskId: 1 }], ["s", ["entries", 1, "id"], 7], ["s", ["list", 2], 30]]) as ReturnType<typeof base>;
    expect((out.docs as Record<string, any>)["pi.live"].run).toEqual({ taskId: 1 });
    expect(out.entries).toEqual([{ id: 1 }, { id: 7 }]);
    expect(out.list).toEqual([1, 2, 30]);
  });

  it("applyOps deletes an object key and splices an array item with d", () => {
    const out = applyOps(deepFreeze(base()), [["d", ["docs", "other"]], ["d", ["list", 1]]]) as ReturnType<typeof base>;
    expect(out.docs).not.toHaveProperty("other");
    expect(out.list).toEqual([1, 3]);
  });

  it("applyOps appends to a string with a", () => {
    const out = applyOps(deepFreeze(base()), [["a", LIVE, "llo"], ["a", LIVE, " world"]]) as any;
    expect(out.docs["pi.live"].generation.message.content[0].text).toBe("hello world");
  });

  it("applyOps drops leading characters with t", () => {
    const out = applyOps(deepFreeze({ s: "abcdef" }), [["t", ["s"], 2]]);
    expect(out).toEqual({ s: "cdef" });
  });

  it("applyOps splices arrays with p and reorders them with m", () => {
    const frozen = deepFreeze(base());
    const out = applyOps(frozen, [["p", ["entries"], 2, 0, [{ id: 3 }]], ["p", ["list"], 0, 1, []], ["m", ["list"], [1, 0]]]) as ReturnType<typeof base>;
    expect(out.entries).toEqual([{ id: 1 }, { id: 2 }, { id: 3 }]);
    expect(out.list).toEqual([3, 2]);
    expect(applyOps([1, 2], [["p", [], 2, 0, [3]], ["m", [], [2, 1, 0]]])).toEqual([3, 2, 1]);
  });

  it("applyOps refuses prototype-polluting path segments", () => {
    for (const seg of ["__proto__", "constructor", "prototype"]) {
      expect(() => applyOps({ a: {} }, [["s", ["a", seg, "x"], 1]])).toThrow(expect.objectContaining(protocolError));
      expect(() => applyOps({ a: {} }, [["s", [seg], 1]])).toThrow(expect.objectContaining(protocolError));
    }
    expect(({} as Record<string, unknown>)["x"]).toBeUndefined();
  });

  it("applyOps refuses an out-of-range index and an unknown op", () => {
    const bad: unknown[][] = [
      [["s", ["list", 3], 1]],
      [["d", ["list", 3]]],
      [["p", ["missing"], 0, 0, []]],
      [["a", ["list", 0], "x"]],
      [["t", ["list", 0], 1]],
      [["m", ["list"], [0]]],
      [["z", ["list"]]],
      [["#", 1, ["list"]]],
    ];
    for (const ops of bad) expect(() => applyOps(base(), ops)).toThrow(expect.objectContaining(protocolError));
  });

  it("applyOps does not mutate its input and applies all ops or none", () => {
    const root = deepFreeze(base());
    const snapshot = structuredClone(root);
    const out = applyOps(root, [["a", LIVE, "x"]]) as any;
    expect(root).toEqual(snapshot);
    expect(out).not.toBe(root);
    expect(out.docs.other).toBe(root.docs && (root.docs as any).other);
    expect(() => applyOps(root, [["s", ["list", 0], 99], ["s", ["list", 9], 1]])).toThrow(expect.objectContaining(protocolError));
    expect(root).toEqual(snapshot);
  });
});

describe("createReplica", () => {
  it("a replica applies a view then ops and tracks the cursor", () => {
    const r = createReplica();
    expect(r.value).toBeNull();
    expect(r.cursor).toBeNull();
    r.view({ epoch: "e1", seq: 1, view: { list: [1] } });
    expect(r.value).toEqual({ list: [1] });
    expect(r.cursor).toEqual({ epoch: "e1", seq: 1 });
    r.ops({ epoch: "e1", seq: 2, ops: [["p", ["list"], 1, 0, [2]]] });
    expect(r.value).toEqual({ list: [1, 2] });
    expect(r.cursor).toEqual({ epoch: "e1", seq: 2 });
  });

  it("a replica refuses an ops frame before any view", () => {
    const r = createReplica();
    expect(() => r.ops({ epoch: "e1", seq: 1, ops: [] })).toThrow(expect.objectContaining(protocolError));
    expect(r.value).toBeNull();
    expect(r.cursor).toBeNull();
  });

  it("a replica refuses an epoch change or a seq gap", () => {
    const r = createReplica();
    r.view({ epoch: "e1", seq: 5, view: { n: 1 } });
    expect(() => r.ops({ epoch: "e2", seq: 6, ops: [["s", ["n"], 2]] })).toThrow(expect.objectContaining(protocolError));
    expect(() => r.ops({ epoch: "e1", seq: 7, ops: [["s", ["n"], 2]] })).toThrow(expect.objectContaining(protocolError));
    expect(() => r.ops({ epoch: "e1", seq: 6, ops: [["s", ["zz", 1], 2]] })).toThrow(expect.objectContaining(protocolError));
    expect(r.value).toEqual({ n: 1 });
    expect(r.cursor).toEqual({ epoch: "e1", seq: 5 });
  });

  it("a view frame mid-stream replaces the replica", () => {
    const r = createReplica();
    r.view({ epoch: "e1", seq: 1, view: { a: 1 } });
    r.ops({ epoch: "e1", seq: 2, ops: [["s", ["a"], 2]] });
    r.view({ epoch: "e2", seq: 9, view: { b: 1 } });
    expect(r.value).toEqual({ b: 1 });
    expect(r.cursor).toEqual({ epoch: "e2", seq: 9 });
    r.ops({ epoch: "e2", seq: 10, ops: [["s", ["b"], 2]] });
    expect(r.value).toEqual({ b: 2 });
  });
});
