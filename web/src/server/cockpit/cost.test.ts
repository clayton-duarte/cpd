import { describe, expect, it } from "vitest";
import { formatCost, readCosts, type CostDeps, type CostFile } from "./cost.ts";

const header = (ts: string, extra: Record<string, unknown> = {}) => JSON.stringify({ type: "session", version: 3, id: "a", timestamp: ts, cwd: "/w", ...extra });
const usage = (total: number) => ({ input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2, reasoning: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total } });
const assistant = (ts: string, total: number) => JSON.stringify({ type: "message", id: "m", parentId: null, timestamp: ts, message: { role: "assistant", content: [], usage: usage(total), stopReason: "stop" } });
const user = (ts: string) => JSON.stringify({ type: "message", id: "u", parentId: null, timestamp: ts, message: { role: "user", content: [] } });
const entry = (type: string, ts: string, total: number) => JSON.stringify({ type, id: "e", parentId: null, timestamp: ts, usage: usage(total) });
const T = (s: number) => `2026-01-01T00:00:${String(s).padStart(2, "0")}.000Z`;
const file = (lines: string[], mtimeMs = 1): CostFile => { const text = lines.join("\n") + "\n"; return { text, mtimeMs, size: text.length }; };

interface Sess { id?: string; runner?: string; role: string; context?: string | null; plan?: string | null; attempt?: string | null; file: string | null; archived_at?: string | null }
interface World { contexts: { id: string; plans: { id: string }[] }[]; sessions: Sess[]; events?: { plan: string; attempt: string; data: { cost_usd: number | null } }[]; files: Record<string, CostFile | null>; durable?: Record<string, { cost: number | null; stamp: string } | null> }
function deps(w: World): CostDeps & { reads: string[] } {
  const reads: string[] = [];
  return {
    reads,
    async cpd(argv) {
      const a = argv.join(" ");
      if (a.startsWith("context list")) return JSON.stringify({ ok: true, contexts: w.contexts });
      if (a.startsWith("session list")) return JSON.stringify({ ok: true, sessions: w.sessions.map((s) => ({ id: "", runner: "pi", context: null, plan: null, attempt: null, archived_at: null, ...s })) });
      if (a.startsWith("events list")) return JSON.stringify({ ok: true, events: (w.events ?? []).map((e, i) => ({ id: i + 1, kind: "attempt-ended", ...e })), next: (w.events ?? []).length });
      throw new Error(`unexpected cpd ${a}`);
    },
    async readFile(path) { reads.push(path); return w.files[path] ?? null; },
    async durableCost(id) { return w.durable?.[id] ?? null; },
  };
}
const one = (id: string, f: CostFile): World => ({ contexts: [{ id: `c-${id}`, plans: [{ id }] }], sessions: [{ role: "lead", plan: id, context: `c-${id}`, file: `/f/${id}` }], files: { [`/f/${id}`]: f } });

describe("cost", () => {
  it("cost sums usage.cost.total over assistant and usage entries without dedup", async () => {
    const f = file([header(T(0)), user(T(1)), assistant(T(1), 0.0123), assistant(T(2), 0.0123), entry("usage", T(3), 0.015), entry("compaction", T(4), 0.1), entry("branch_summary", T(5), 0.2)]);
    const costs = await readCosts(deps(one("sum1", f)));
    expect(costs["sum1"]).toBeCloseTo(0.0123 * 2 + 0.015 + 0.1 + 0.2, 10);
  });
  it("cost skips a fork entry stamped before the header", async () => {
    const f = file([header(T(10), { parentSession: "/p.jsonl" }), assistant(T(5), 9), entry("usage", T(9), 9), assistant(T(11), 0.5)]);
    expect((await readCosts(deps(one("fork1", f))))["fork1"]).toBeCloseTo(0.5, 10);
  });
  it("cost keeps the last good value when a line is malformed", async () => {
    const w = one("bad1", file([header(T(0)), assistant(T(1), 0.4)], 1));
    expect((await readCosts(deps(w)))["bad1"]).toBeCloseTo(0.4, 10);
    w.files["/f/bad1"] = file([header(T(0)), assistant(T(1), 0.4), "{not json", assistant(T(2), 5)], 2);
    expect((await readCosts(deps(w)))["bad1"]).toBeCloseTo(0.4, 10);
  });
  it("cost caches a file by mtime and size", async () => {
    const a = file([header(T(0)), assistant(T(1), 0.1)], 7);
    const w = one("cache1", a);
    expect((await readCosts(deps(w)))["cache1"]).toBeCloseTo(0.1, 10);
    // same mtime and size, different content: the cached cost stands
    const same = a.text.replace("0.1", "0.2");
    expect(same.length).toBe(a.size);
    w.files["/f/cache1"] = { text: same, mtimeMs: 7, size: a.size };
    expect((await readCosts(deps(w)))["cache1"]).toBeCloseTo(0.1, 10);
    // a new mtime reparses
    w.files["/f/cache1"] = { text: same, mtimeMs: 8, size: a.size };
    expect((await readCosts(deps(w)))["cache1"]).toBeCloseTo(0.2, 10);
  });
  it("cost totals a Plan from lead sessions and attempts", async () => {
    const w: World = {
      contexts: [{ id: "c-plan", plans: [{ id: "pl-plan" }] }],
      sessions: [
        { role: "lead", plan: "pl-plan", context: "c-plan", file: "/p/lead" },
        { role: "lead", plan: "pl-plan", context: "c-plan", file: "/p/lead-old", archived_at: "2026-01-02T00:00:00.000Z" },
        { role: "worker", plan: "pl-plan", attempt: "a1", file: "/p/w1" },
        { role: "worker", plan: "pl-plan", attempt: "a2", file: "/p/w2" },
        { role: "lead", plan: "other", context: "c-other", file: "/p/elsewhere" },
      ],
      events: [{ plan: "pl-plan", attempt: "a1", data: { cost_usd: 2 } }, { plan: "pl-plan", attempt: "a2", data: { cost_usd: null } }],
      files: {
        "/p/lead": file([header(T(0)), assistant(T(1), 0.5)]),
        "/p/lead-old": file([header(T(0)), assistant(T(1), 0.25)]),
        "/p/w1": file([header(T(0)), assistant(T(1), 99)]),
        "/p/w2": file([header(T(0)), assistant(T(1), 1)]),
        "/p/elsewhere": file([header(T(0)), assistant(T(1), 50)]),
      },
    };
    expect((await readCosts(deps(w)))["pl-plan"]).toBeCloseTo(0.5 + 0.25 + 2 + 1, 10);
  });
  it("cost totals a Context from its Plans and manager sessions", async () => {
    const w: World = {
      contexts: [{ id: "c-ctx", plans: [{ id: "pl-ctx1" }, { id: "pl-ctx2" }] }],
      sessions: [
        { role: "lead", plan: "pl-ctx1", context: "c-ctx", file: "/c/l1" },
        { role: "lead", plan: "pl-ctx2", context: "c-ctx", file: "/c/l2" },
        { role: "manager", context: "c-ctx", file: "/c/m" },
        { role: "manager", context: "c-ctx", file: "/c/m-old", archived_at: "2026-01-02T00:00:00.000Z" },
        { role: "worker", plan: "pl-ctx1", attempt: "x", context: "c-ctx", file: "/c/wx" },
      ],
      events: [{ plan: "pl-ctx1", attempt: "x", data: { cost_usd: 3 } }],
      files: {
        "/c/l1": file([header(T(0)), assistant(T(1), 1)]),
        "/c/l2": file([header(T(0)), assistant(T(1), 2)]),
        "/c/m": file([header(T(0)), assistant(T(1), 10)]),
        "/c/m-old": file([header(T(0)), assistant(T(1), 20)]),
        "/c/wx": file([header(T(0)), assistant(T(1), 999)]),
      },
    };
    const costs = await readCosts(deps(w));
    expect(costs["pl-ctx1"]).toBeCloseTo(4, 10);
    expect(costs["pl-ctx2"]).toBeCloseTo(2, 10);
    expect(costs["c-ctx"]).toBeCloseTo(4 + 2 + 10 + 20, 10);
  });
  it("counts a durable Lead and Manager in the plan and context totals", async () => {
    const w: World = {
      contexts: [{ id: "c-dur", plans: [{ id: "pl-dur" }] }],
      sessions: [
        { id: "l-dur", runner: "durable", role: "lead", plan: "pl-dur", context: "c-dur", file: null },
        { id: "m-dur", runner: "durable", role: "manager", context: "c-dur", file: null },
        { id: "l-pi", role: "lead", plan: "pl-dur", context: "c-dur", file: "/d/lead" },
      ],
      files: { "/d/lead": file([header(T(0)), assistant(T(1), 1)]) },
      durable: { "l-dur": { cost: 0.75, stamp: "s1" }, "m-dur": { cost: 2.5, stamp: "s1" } },
    };
    const costs = await readCosts(deps(w));
    expect(costs["pl-dur"]).toBeCloseTo(1.75, 10);
    expect(costs["c-dur"]).toBeCloseTo(4.25, 10);
  });
  it("a durable session with no store is not counted", async () => {
    const w: World = {
      contexts: [{ id: "c-nos", plans: [{ id: "pl-nos" }] }],
      sessions: [
        { id: "l-nos", runner: "durable", role: "lead", plan: "pl-nos", context: "c-nos", file: null },
        { id: "m-nos", runner: "durable", role: "manager", context: "c-nos", file: null },
      ],
      files: {},
      durable: { "l-nos": null, "m-nos": { cost: 2, stamp: "s1" } },
    };
    const costs = await readCosts(deps(w));
    expect(costs["pl-nos"]).toBeUndefined();
    expect(costs["c-nos"]).toBeCloseTo(2, 10);
  });
  it("reads a durable store again when its stamp changes", async () => {
    const w: World = {
      contexts: [{ id: "c-stp", plans: [{ id: "pl-stp" }] }],
      sessions: [{ id: "l-stp", runner: "durable", role: "lead", plan: "pl-stp", context: "c-stp", file: null }],
      files: {},
      durable: { "l-stp": { cost: 1, stamp: "s1" } },
    };
    expect((await readCosts(deps(w)))["pl-stp"]).toBeCloseTo(1, 10);
    w.durable = { "l-stp": { cost: 3, stamp: "s2" } };
    expect((await readCosts(deps(w)))["pl-stp"]).toBeCloseTo(3, 10);
  });
  it("formatCost formats the thresholds like the TUI", () => {
    const table: [number, string][] = [
      [0, "$0.000"], [Number.NaN, "$0.000"], [-3, "$0.000"], [0.0004, "$0.000"],
      [0.0123, "$0.012"], [1.2344, "$1.234"], [0.0625, "$0.062"], [0.125, "$0.125"],
      [12.344, "$12.34"], [12.125, "$12.12"], [123.44, "$123.4"], [123.25, "$123.2"],
      [999.96, "$1.00k"], [1234, "$1.23k"], [1125, "$1.12k"], [12345, "$12.3k"], [123456, "$123k"], [999501, "$1000k"],
    ];
    expect(table.map(([n]) => formatCost(n))).toEqual(table.map(([, s]) => s));
  });
});
