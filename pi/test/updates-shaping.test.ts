import assert from "node:assert/strict";
import { test } from "node:test";
import { parsePending, pendingSection, type PendingPlan } from "../lib/updates.ts";

// ctx-explore, commands.md §15.7 "Managers: compressed digest", §3.12 `context pending`: a Manager's turn-start
// digest lists a Lead-less shaping Plan (phase `shaping`, `lead_session` null; air-d19, air-d22) in `Pending work:`
// with its plan-level explore Job counts, like any Plan with no Workflow. The digest reads that Plan's `lead_session`
// from `cpd context pending`: `parsePending` surfaces it as `leadSession`. The literals below are plain JSON, never a
// typed `PendingPlan`, so this file compiles before the implementation.

/** A plan-level explore Job of `cpd context pending`, as plain JSON. */
const job = (key: string, kind: string, status: string) => ({ key, kind, status, tier: kind === "scout" ? "H3" : "S1", title: `Title ${key}`, touches: [], waiting_on: [] });

/** What `cpd context pending` prints for a Lead-less shaping Plan: `lead_session` null, phase `shaping`, no Workflow. */
const shaping = (jobs: ReturnType<typeof job>[]) => ({ id: "pay-4522", title: "Retry failed payouts", phase: "shaping", lead_session: null, jobs, workflows: [] });

/** A Workflow of `cpd context pending`, as plain JSON. */
const workflow = (key: string, status: string) => ({ key, repo: "/r", base: "main", status, lead_branch: `cpd/pay-4521/${key}`, pull_request: null, jobs: [{ key: `${key}-w`, status: "queued", title: "W", touches: [] }] });

/** A Plan with a Lead, as plain JSON: `lead_session` is the Lead Session's id. */
const leadPlan = () => ({ id: "pay-4521", title: "Lead plan", phase: "dispatch", lead_session: "596d007f-6775-4134-b77a-b356cc523842", jobs: [], workflows: [workflow("api", "dispatch")] });

/** `parsePending`'s plans, read past the current `PendingPlan`: `leadSession` is the field the digest needs. */
type WithLead = PendingPlan & { leadSession: string | null };
const plans = (out: Record<string, unknown>) => parsePending(out) as unknown as WithLead[];

test("digest pending work lists a shaping plan with its job counts", () => {
  const out = { ok: true, context: "payments", plans: [shaping([job("scan", "scout", "running"), job("dig", "research", "queued")])] };
  // The Plan is Lead-less (`lead_session` null): the digest reads that to treat it as a shaping Plan.
  assert.equal(plans(out)[0]!.leadSession, null);
  // Its line is its plan-level explore Job counts, with no Workflow segment: `- <id> (<phase>): jobs <counts>`.
  assert.deepEqual(pendingSection(parsePending(out)), ["Pending work:", "- pay-4522 (shaping): jobs 1 running, 1 queued"]);
});

test("digest pending work accepts a null lead session", () => {
  const out = { ok: true, context: "payments", plans: [shaping([job("scan", "scout", "queued")]), leadPlan()] };
  const parsed = plans(out);
  assert.equal(parsed[0]!.leadSession, null, "a Lead-less shaping Plan");
  assert.equal(parsed[1]!.leadSession, "596d007f-6775-4134-b77a-b356cc523842", "a Plan with a Lead");
  // Both are listed: the shaping Plan by its Job counts, the Lead Plan by its Workflow.
  assert.deepEqual(pendingSection(parsePending(out)), [
    "Pending work:",
    "- pay-4522 (shaping): jobs 1 queued",
    "- pay-4521 (dispatch): Workflow api 1 queued",
  ]);
});
