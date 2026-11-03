import { describe, expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { createReadModel } from "./readmodel.ts";
import { WebError } from "../../protocol/domain.ts";

type Obj = Record<string, unknown>;

const context = (id: string, extra: Obj = {}): Obj => ({ id, title: `Title ${id}`, archived_at: null, records: 2, open_questions: 1, ...extra });
const plan = (id: string, ctx: string, extra: Obj = {}): Obj => ({
  id, context: ctx, title: `Plan ${id}`, phase: "dispatch", archived_at: null, created_at: "2026-01-01T00:00:00Z",
  lead_session: null, counts: { draft: 0, queued: 1, running: 2, merged: 3, failed: 0, cancelled: 0 }, ...extra,
});
const session = (id: string, extra: Obj = {}): Obj => ({
  id, key: null, name: null, runner: "pi", role: "manager", state: "idle", context: "c1", plan: null, plan_lead: false,
  archived_at: null, file: null, tier: null, title: null, cwd: null, ...extra,
});
const item = (id: number | null, extra: Obj = {}): Obj => ({
  id, kind: "unread", label: "a label", urgency: "white", opened_at: "2026-01-01T00:00:00Z", age_seconds: 5,
  context: null, plan: null, job: null, session: null, url: null, snoozed_until: null, dismissed_at: null, claimed_at: null, ...extra,
});

interface Replies { context?: Obj[]; plan?: Obj[]; session?: Obj[]; inbox?: Obj[]; show?: Obj }

function fakeCpd(r: Replies, overrides: Record<string, () => Promise<Obj>> = {}): { cpd: Cpd; calls: string[] } {
  const calls: string[] = [];
  const cpd: Cpd = {
    async json(args) {
      const key = args.join(" ");
      calls.push(key);
      const over = overrides[key];
      if (over !== undefined) return over();
      switch (key) {
        case "context list": return { ok: true, contexts: r.context ?? [], events_cursor: 10 };
        case "plan list": return { ok: true, plans: r.plan ?? [], events_cursor: 8 };
        case "session list": return { ok: true, sessions: r.session ?? [], events_cursor: 12 };
        case "inbox list": return { ok: true, items: r.inbox ?? [], engine: { running: true }, hidden: { claimed: 1, dismissed: 2, snoozed: 3 }, events_cursor: 9 };
        default:
          if (key.startsWith("plan show") && r.show !== undefined) return r.show;
          throw new Error(`unexpected ${key}`);
      }
    },
  };
  return { cpd, calls };
}

const planShow = (): Obj => ({
  ok: true,
  plan: { id: "p1", context: "c1", title: "Plan p1", phase: "dispatch", archived_at: null, lead_session: "s-lead" },
  counts: { draft: 0, queued: 1, running: 1, merged: 0, failed: 0, cancelled: 0 },
  jobs: [
    { key: "a", title: "Job A", kind: "code", status: "merged", tier: "S1", needs: [], attempts: [] },
    { key: "b", title: "Job B", kind: "code", status: "running", tier: "S2", needs: ["a"], attempts: [{ id: "at1", number: 1, tier: "S2", status: "running", outcome: null, cause: null, worker_session: "w1" }] },
    { key: "x", title: "Explore", kind: "explore", status: "queued", tier: "S1", attempts: [] },
  ],
  workflows: [{ key: "w", title: "WF", repo: "r", base: "main", status: "open", needs: [], jobs: ["a", "b"], pr: null, pull_request: null }],
  review: null,
  escalations: [{}],
  events_cursor: 20,
});

describe("readmodel", () => {
  it("context tree groups plans and sessions under their contexts", async () => {
    const { cpd } = fakeCpd({
      context: [context("c1"), context("c2")],
      plan: [plan("p1", "c1", { lead_session: "s-lead" })],
      session: [
        session("s-mgr", { role: "manager" }),
        session("s-w1", { role: "worker", plan: "p1", key: "c1:p1:j1" }),
        session("s-lead", { role: "lead", plan: "p1", plan_lead: true, key: "c1:p1" }),
      ],
    });
    const { tree } = await createReadModel(cpd).loadAll(null);
    expect(tree.contexts.map((c) => c.id)).toEqual(["c1", "c2"]);
    const c1 = tree.contexts[0]!;
    expect(c1.sessions.map((s) => s.ref.id)).toEqual(["s-mgr"]);
    expect(c1.plans.map((p) => p.id)).toEqual(["p1"]);
    expect(c1.plans[0]!.sessions.map((s) => s.ref.id)).toEqual(["s-lead", "s-w1"]);
    expect(c1.plans[0]!.sessions[1]!.jobKey).toBe("j1");
    expect(tree.contexts[1]!.plans).toEqual([]);
    expect(tree.cursor).toBe(8);
  });

  it("session state dot derives from state and open inbox items", async () => {
    const { cpd } = fakeCpd({
      context: [context("c1")],
      plan: [],
      session: [
        session("busy", { state: "working" }),
        session("waiting", { state: "waiting" }),
        session("quiet", { state: "idle" }),
        session("snoozed", { state: "idle" }),
      ],
      inbox: [
        item(1, { session: "busy", urgency: "red" }),
        item(2, { session: "waiting", urgency: "cyan" }),
        item(3, { session: "waiting", urgency: "red" }),
        item(4, { session: "snoozed", urgency: "red", snoozed_until: "2026-01-02T00:00:00Z" }),
      ],
    });
    const { tree } = await createReadModel(cpd).loadAll(null);
    const dots = Object.fromEntries(tree.contexts[0]!.sessions.map((s) => [s.ref.id, s.dot]));
    expect(dots["busy"]).toEqual({ state: "working", attention: null });
    expect(dots["waiting"]).toEqual({ state: "waiting", attention: "red" });
    expect(dots["quiet"]).toEqual({ state: "idle", attention: null });
    expect(dots["snoozed"]).toEqual({ state: "idle", attention: null });
  });

  it("plan view lists workflows with their jobs, needs and statuses", async () => {
    const { cpd, calls } = fakeCpd({ show: planShow() });
    const update = await createReadModel(cpd).loadPanes(new Set(["plan"]), "p1");
    expect(calls).toEqual(["plan show --plan p1"]);
    const view = update.plan!;
    expect(view.planId).toBe("p1");
    expect(view.openEscalations).toBe(1);
    expect(view.cursor).toBe(20);
    expect(view.workflows.map((w) => [w.key, w.jobKeys])).toEqual([["w", ["a", "b"]]]);
    expect(view.jobs.map((j) => [j.key, j.status, j.needs, j.workflow])).toEqual([
      ["a", "merged", [], "w"],
      ["b", "running", ["a"], "w"],
      ["x", "queued", [], null],
    ]);
    expect(view.jobs[1]!.attempts[0]!.workerSession).toBe("w1");
  });

  it("attention inbox rows carry kind and available actions", async () => {
    const { cpd } = fakeCpd({
      inbox: [
        item(7, { kind: "review-requested", urgency: "yellow" }),
        item(8, { kind: "deploy", urgency: "red" }),
        item(9, { kind: "unread" }),
      ],
    });
    const { inbox } = await createReadModel(cpd).loadAll(null);
    expect(inbox.openCount).toBe(3);
    expect(inbox.engineRunning).toBe(true);
    expect(inbox.hidden).toEqual({ claimed: 1, dismissed: 2, snoozed: 3 });
    expect(inbox.rows.map((r) => r.kind)).toEqual(["review-requested", "deploy", "unread"]);
    const labels = (i: number) => inbox.rows[i]!.actions.map((a) => a.label);
    expect(labels(0)).toEqual(["Claim", "Snooze 15m", "Snooze 1h", "Snooze until tomorrow", "Dismiss"]);
    expect(labels(1)).toEqual(["Approve", "Snooze 15m", "Snooze 1h", "Snooze until tomorrow", "Dismiss"]);
    expect(inbox.rows[2]!.actions.map((a) => a.action)).toEqual([
      { kind: "inbox.snooze", itemId: 9, duration: "15m" },
      { kind: "inbox.snooze", itemId: 9, duration: "1h" },
      { kind: "inbox.snooze", itemId: 9, duration: "tomorrow" },
      { kind: "inbox.dismiss", itemId: 9 },
    ]);
  });

  it("a failing cpd command yields a typed error and keeps the previous snapshot", async () => {
    let fail = false;
    const { cpd } = fakeCpd({ context: [context("c1")] }, {
      "plan list": async () => {
        if (fail) throw new WebError("cpd_failed", "boom", { cpdCode: "x", exitCode: 1 });
        return { ok: true, plans: [], events_cursor: 8 };
      },
    });
    const model = createReadModel(cpd);
    const first = await model.loadAll(null);
    fail = true;
    await expect(model.loadPanes(new Set(["tree"]), null)).rejects.toMatchObject({ name: "WebError", code: "cpd_failed" });
    expect(first.tree.contexts.map((c) => c.id)).toEqual(["c1"]);
  });

  it("malformed cpd JSON is rejected at the boundary", async () => {
    const { cpd } = fakeCpd({ context: [{ id: "c1", title: "t", archived_at: null, records: "many", open_questions: 0 }] });
    await expect(createReadModel(cpd).loadAll(null)).rejects.toMatchObject({ name: "WebError", code: "cpd_malformed_json" });
  });
});
