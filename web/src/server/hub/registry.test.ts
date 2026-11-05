import { describe, expect, it } from "vitest";
import { WebError } from "../../protocol/domain.ts";
import { createRegistry, SOCKET_PATH_MAX, socketPathOf } from "./registry.ts";

type Deps = Parameters<typeof createRegistry>[0];
type Snapshot = ReturnType<Deps["snapshot"]>;

interface Row { id: string; runner: "pi" | "durable"; file?: string; tier?: string; planId?: string | null }

const node = (r: Row) => ({
  ref: { id: r.id, key: `ctx:${r.id}`, runner: r.runner, role: "worker", state: "idle", title: null, ...(r.file === undefined ? {} : { file: r.file }), ...(r.tier === undefined ? {} : { tier: r.tier }) },
  contextId: "ctx",
  planId: r.planId ?? null,
  jobKey: null,
  archived: false,
  dot: { state: "idle", attention: null },
});

const snapshotOf = (contextRows: Row[], planRows: Row[]): Snapshot =>
  ({
    tree: {
      cursor: 1,
      contexts: [
        {
          id: "ctx", title: "ctx", archived: false, records: 0, openQuestions: 0,
          sessions: contextRows.map(node),
          plans: [{ id: "p1", contextId: "ctx", title: "p", phase: "plan", archived: false, counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 }, leadSessionId: null, sessions: planRows.map((r) => node({ ...r, planId: "p1" })) }],
        },
      ],
    },
    plan: null,
    inbox: null,
  }) as unknown as Snapshot;

const thrown = (fn: () => unknown): unknown => {
  try {
    fn();
  } catch (e) {
    return e;
  }
  return undefined;
};

describe("registry", () => {
  it("resolves a session id to runner, file and socket path from the cockpit snapshot", () => {
    const snap = snapshotOf(
      [{ id: "pi-1", runner: "pi", file: "/s/pi-1.jsonl", tier: "S3" }],
      [{ id: "dur-1", runner: "durable", tier: "S1" }],
    );
    const registry = createRegistry({ snapshot: () => snap, cpdHome: "/home/x/.cpd" });

    const pi = registry.resolve("pi-1");
    expect(pi.ref.runner).toBe("pi");
    expect(pi.file).toBe("/s/pi-1.jsonl");
    expect(pi.socketPath).toBeUndefined();
    expect(pi.tierOptions).toEqual(["S1", "S3"]);

    const dur = registry.resolve("dur-1");
    expect(dur.ref.runner).toBe("durable");
    expect(dur.socketPath).toBe("/home/x/.cpd/run/host/dur-1.sock");
    expect(dur.socketPath).toBe(socketPathOf("/home/x/.cpd", "dur-1"));
    expect(dur.file).toBeUndefined();
    expect(SOCKET_PATH_MAX).toBe(107);
  });

  it("an unknown session id yields session_not_found", () => {
    const registry = createRegistry({ snapshot: () => snapshotOf([{ id: "a", runner: "pi", file: "/f" }], []), cpdHome: "/h" });
    const e = thrown(() => registry.resolve("nope"));
    expect(e).toBeInstanceOf(WebError);
    expect((e as WebError).code).toBe("session_not_found");
    expect((e as WebError).details).toEqual({ sessionId: "nope" });

    const empty = createRegistry({ snapshot: () => ({ tree: null, plan: null, inbox: null }), cpdHome: "/h" });
    expect((thrown(() => empty.resolve("a")) as WebError).code).toBe("session_not_found");
  });

  it("resolve returns a pi session without a file", () => {
    const registry = createRegistry({ snapshot: () => snapshotOf([], [{ id: "pi-nofile", runner: "pi" }]), cpdHome: "/h" });
    const r = registry.resolve("pi-nofile");
    expect(r).toEqual({ ref: expect.objectContaining({ id: "pi-nofile", runner: "pi" }), tierOptions: expect.any(Array) });
    expect("file" in r).toBe(false);
  });
});
