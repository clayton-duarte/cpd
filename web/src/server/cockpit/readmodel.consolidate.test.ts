import { expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { createReadModel } from "./readmodel.ts";

type Obj = Record<string, unknown>;

const plan = (id: string, extra: Obj = {}): Obj => ({
  id, context: "c1", title: `Plan ${id}`, phase: "dispatch", archived_at: null, created_at: "2026-01-01T00:00:00Z",
  lead_session: null, counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 }, ...extra,
});
const session = (id: string, extra: Obj = {}): Obj => ({
  id, key: null, name: null, runner: "pi", role: "manager", state: "idle", context: "c1", plan: null, plan_lead: false,
  archived_at: null, file: null, tier: null, title: null, cwd: null, ...extra,
});
const item = (id: number, extra: Obj = {}): Obj => ({
  id, kind: "dialog", label: "a label", urgency: "red", opened_at: "2026-01-01T00:00:00Z", age_seconds: 5,
  context: null, plan: null, job: null, session: null, url: null, snoozed_until: null, dismissed_at: null, claimed_at: null, ...extra,
});

function fakeCpd(sessions: Obj[], plans: Obj[], items: Obj[]): Cpd {
  return {
    async json(args) {
      switch (args.join(" ")) {
        case "context list": return { ok: true, contexts: [{ id: "c1", title: "C1", archived_at: null, records: 0, open_questions: 0 }], events_cursor: 10 };
        case "plan list": return { ok: true, plans, events_cursor: 8 };
        case "session list": return { ok: true, sessions, events_cursor: 12 };
        case "inbox list": return { ok: true, items, engine: { running: true }, hidden: { claimed: 0, dismissed: 0, snoozed: 0 }, events_cursor: 9 };
        default: throw new Error(`unexpected ${args.join(" ")}`);
      }
    },
  };
}

it("readmodel resolves a missing sessionId to the plan lead or the context manager", async () => {
  const sessions = [
    session("OLD-LEAD", { role: "lead", plan: "p2", plan_lead: true, archived_at: "2026-01-02T00:00:00Z" }),
    session("L2", { role: "lead", plan: "p2", plan_lead: true }),
    session("OLD-MGR", { archived_at: "2026-01-02T00:00:00Z" }),
    session("M1"),
  ];
  const plans = [plan("p1", { lead_session: "LEAD-OF-P1" }), plan("p2")];
  const items = [
    item(1, { plan: "p1" }), // the Plan's lead_session
    item(2, { plan: "p2" }), // the live (not archived) plan_lead session
    item(3, { kind: "question", context: "c1" }), // the live Manager of the Context
    item(4, { kind: "curate-retire", context: "c1" }), // same for curate-retire
    item(5, { kind: "unread", context: "c1" }), // no manager fallback for other kinds
    item(6, { plan: "p1", session: "OWN" }), // the row's own session wins
    item(7, { kind: "question", context: "nope" }), // nothing to resolve to
  ];
  const { inbox } = await createReadModel(fakeCpd(sessions, plans, items)).loadAll(null);
  expect(inbox.rows.map((r) => [r.id, r.sessionId])).toEqual([
    [1, "LEAD-OF-P1"],
    [2, "L2"],
    [3, "M1"],
    [4, "M1"],
    [5, null],
    [6, "OWN"],
    [7, null],
  ]);
});
