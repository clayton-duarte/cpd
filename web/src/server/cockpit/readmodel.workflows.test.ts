import { expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { buildPlanView, buildTree, createReadModel, parsePlanList, parsePlanShow } from "./readmodel.ts";

type Obj = Record<string, unknown>;

const counts = { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 };
const planRow = (id: string, extra: Obj = {}): Obj => ({
  id, context: "c1", title: `Plan ${id}`, phase: "dispatch", archived_at: null, created_at: "2026-01-01T00:00:00Z",
  lead_session: null, counts, ...extra,
});
const listReply = (plans: Obj[]): Obj => ({ ok: true, plans, events_cursor: 1 });

const open = {
  key: "mv", repo: "/r/mv", status: "open",
  pr: { number: 7, url: "https://github.com/acme/api/pull/7" },
  pull_request: { repo: "acme/api", number: 7, state: "open", checks: "passed", mergeable: "conflicting", title: "x" },
};
const waiting = { key: "later", repo: "/r/later", status: "waiting", pr: null, pull_request: null };

it("parsePlanList keeps each plan workflows with pr state, checks and mergeable", () => {
  const [p] = parsePlanList(listReply([planRow("p1", { workflows: [open, waiting] })]));
  expect(p!.workflows).toEqual([
    { key: "mv", status: "open", repo: "/r/mv", pr: { number: 7, url: "https://github.com/acme/api/pull/7", state: "open", checks: "passed", mergeable: "conflicting" } },
    { key: "later", status: "waiting", repo: "/r/later", pr: null },
  ]);
});

it("parsePlanList gives a plan without workflows an empty list", () => {
  const [p] = parsePlanList(listReply([planRow("p1")]));
  expect(p!.workflows).toEqual([]);
});

it("buildTree puts a plan workflows on its PlanNode", () => {
  const workflows = [{ key: "mv", status: "open", repo: "/r/mv", pr: { number: 7, url: "u", state: "open", checks: "passed", mergeable: "clean" } }];
  const tree = buildTree({
    contexts: [{ id: "c1", title: "C1", archived_at: null, records: 0, open_questions: 0 }],
    plans: [{ id: "p1", context: "c1", title: "P", phase: "dispatch", archived_at: null, created_at: "2026-01-01T00:00:00Z", counts, lead_session: null, workflows }],
    sessions: [], inbox: [], cursor: 1,
  });
  expect(tree.contexts[0]!.plans[0]!.workflows).toEqual(workflows);
});

it("buildPlanView carries pr.posted from plan show", async () => {
  const wf = (key: string, posted: boolean) => ({ ...open, key, jobs: [], needs: [], title: key, base: "main", pr: { ...open.pr, posted } });
  const show: Obj = {
    ok: true,
    plan: { id: "p1", context: "c1", title: "P", phase: "dispatch", archived_at: null, lead_session: null },
    counts, jobs: [], workflows: [wf("a", true), wf("b", false)], review: null, escalations: [], reviewed_pr: null, events_cursor: 2,
  };
  const view = buildPlanView(parsePlanShow(show));
  expect(view.workflows.map((w) => w.pr?.posted)).toEqual([true, false]);
  const cpd: Cpd = { async json() { return show; } };
  const loaded = await createReadModel(cpd).loadPanes(new Set(["plan"]), "p1");
  expect(loaded.plan!.workflows.map((w) => w.pr?.posted)).toEqual([true, false]);
});
