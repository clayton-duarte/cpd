import { describe, expect, it } from "vitest";
import type { PlanViewModel } from "../protocol/cockpit.ts";
import type { SessionRef, TranscriptItem } from "../protocol/domain.ts";
import { buildPayload, ledgerActionsOf } from "./completion-payload.ts";

const bash = (id: string, command: unknown): TranscriptItem => ({ kind: "tool", id: `tool:${id}`, callId: id, name: "bash", args: { command }, status: "done" });
const user = (id: string, text: string): TranscriptItem => ({ kind: "user", id, text, attachments: [] });
const asst = (id: string, ...texts: string[]): TranscriptItem => ({ kind: "assistant", id, blocks: texts.map((text) => ({ type: "text" as const, text })), streaming: false });
const ref: SessionRef = { id: "s1", key: "c:p", runner: "pi", role: "lead", state: "idle", title: null };

describe("ledgerActionsOf", () => {
  it("ledgerActionsOf lists the cpd state commands of bash items", () => {
    const items = [
      bash("1", "cpd context decide --context c 'use x'"),
      bash("2", "cpd plan new --context c"),
      bash("3", "cpd ask --session s 'why?'"),
      bash("4", "cpd workflow add --plan p"),
      bash("5", "cd /tmp && cpd job add --plan p x"),
      bash("6", "cpd context note a"),
      bash("7", "cpd plan lock --plan p"),
      bash("8", "cpd session send s hello"),
    ];
    expect(ledgerActionsOf(items)).toEqual([
      "cpd context decide --context c 'use x'",
      "cpd plan new --context c",
      "cpd ask --session s 'why?'",
      "cpd workflow add --plan p",
      "cpd job add --plan p x",
      "cpd context note a",
      "cpd plan lock --plan p",
      "cpd session send s hello",
    ]);
  });
  it("ledgerActionsOf ignores other commands", () => {
    const items = [bash("1", "ls -la"), bash("2", "cpd plan show --plan p"), bash("3", "cpd status"), bash("4", 42), user("u1", "cpd context decide x"), asst("a1", "cpd ask y")];
    expect(ledgerActionsOf(items)).toEqual([]);
    const long = `cpd context decide ${"z".repeat(300)}`;
    expect(ledgerActionsOf([bash("5", long)])).toEqual([long.slice(0, 160)]);
  });
});

const planVm = {
  planId: "p1", contextId: "c1", title: "Plan one", phase: "execute",
  counts: { jobs: 2 },
  review: null,
  openEscalations: 3,
  workflows: [
    { key: "w1", title: "W one", repo: "r", base: "main", status: "open", needs: [], jobKeys: ["j1"], pr: { number: 1, url: "u", state: "OPEN", checks: null, mergeable: null } },
    { key: "w2", title: "W two", repo: "r", base: "main", status: "merged", needs: [], jobKeys: [], pr: null },
  ],
  jobs: [{ key: "j1", title: "J one", kind: "code", status: "running", tier: "S1", needs: [], workflow: "w1", attempts: [] }],
  cursor: 7,
} as unknown as PlanViewModel;

describe("buildPayload", () => {
  it("buildPayload keeps the last 40 transcript items", () => {
    const items = Array.from({ length: 55 }, (_, i) => user(`u${i}`, `m${i}`));
    const p = buildPayload({ ref, hasOpenDialog: false, plan: null, items });
    expect(p.transcript).toHaveLength(40);
    expect(p.transcript[0]).toEqual({ kind: "user", text: "m15" });
    expect(p.transcript[39]).toEqual({ kind: "user", text: "m54" });
  });
  it("buildPayload truncates each text to 600 characters", () => {
    const p = buildPayload({ ref, hasOpenDialog: false, plan: null, items: [user("u1", "a".repeat(900)), asst("a1", "b".repeat(400), "c".repeat(400)), bash("1", "d".repeat(900))] });
    for (const t of p.transcript) expect(t.text.length).toBeLessThanOrEqual(600);
    expect(p.transcript[0]?.text).toBe("a".repeat(600));
    expect(p.transcript[1]?.text.startsWith("b".repeat(400))).toBe(true);
  });
  it("buildPayload skips fold compaction and orphan items", () => {
    const items: TranscriptItem[] = [
      user("u1", "hello"),
      { kind: "fold", id: "f1", foldKind: "cpd", title: "t", body: "b", defaultOpen: false },
      { kind: "compaction", id: "c1", summary: "s" },
      { kind: "orphanResult", id: "orphan:x", callId: "x", name: "bash", result: { text: "r", isError: false } },
      asst("a1", "hi"),
      bash("1", "echo ok"),
    ];
    const p = buildPayload({ ref, hasOpenDialog: false, plan: null, items });
    expect(p.transcript.map((t) => t.kind)).toEqual(["user", "assistant", "tool"]);
    expect(p.transcript[2]?.text).toBe("bash echo ok");
  });
  it("buildPayload maps the plan facts and session", () => {
    const p = buildPayload({ ref, hasOpenDialog: true, plan: planVm, items: [user("u1", "x"), bash("1", "cpd plan lock --plan p1")] });
    expect(p.session).toEqual({ id: "s1", key: "c:p", role: "lead", state: "idle", hasOpenDialog: true });
    expect(p.plan).toEqual({
      id: "p1", title: "Plan one", phase: "execute", counts: { jobs: 2 },
      workflows: [{ key: "w1", title: "W one", status: "open", prState: "OPEN" }, { key: "w2", title: "W two", status: "merged", prState: null }],
      jobs: [{ key: "j1", kind: "code", status: "running", title: "J one" }],
      hasPendingReview: false,
      openEscalations: 3,
    });
    expect(p.ledgerActions).toEqual(["cpd plan lock --plan p1"]);
  });
  it("buildPayload accepts a null plan", () => {
    const p = buildPayload({ ref, hasOpenDialog: false, plan: null, items: [user("u1", "x")] });
    expect(p.plan).toBeNull();
    expect(p.transcript).toEqual([{ kind: "user", text: "x" }]);
  });
});
