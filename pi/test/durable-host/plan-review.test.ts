import assert from "node:assert/strict";
import { test } from "node:test";
import { createPlanReview, type PlanReviewPort } from "../../durable-host/plan-review.ts";
import * as held from "../../durable-host/held.ts";
import { createDialogs } from "../../durable-host/dialogs.ts";

// Contract §6.15 `plan-review.ts`: the flow is tested with fake `readText`, `write` and `entries` (a literal with a
// pending `cpd-plan-review` entry and a newer one), fake `cpd`, and the real `held` and `dialogs` so the dialog steps
// are visible. Every test drives the stub through `createPlanReview` (or a dialog), so each fails on 'not
// implemented'; the fakes record what the implemented flow would write, run and broadcast.

const entryOf = (review: string, status: string) => ({
  type: "custom",
  customType: "cpd-plan-review",
  data: { plan: "p1", review, surface: "chat", status },
});
// An entry the adapter sees as another Plan (skipped by reviewsFromEntries).
const otherPlan = (review: string, status: string) => ({
  type: "custom",
  customType: "cpd-plan-review",
  data: { plan: "p2", review, surface: "chat", status },
});

const makePort = (entries: readonly unknown[] = []) => {
  const written: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }[] = [];
  const cpd: string[][] = [];
  const read: string[] = [];
  const broadcasted: Record<string, unknown>[] = [];
  let stateChanges = 0;
  const dialogs = createDialogs({ held, broadcast: (m) => broadcasted.push(m), stateChanged: () => stateChanges++ });
  const port: PlanReviewPort = {
    plan: "p1",
    cpd: async (args) => {
      cpd.push([...args]);
      return { ok: true };
    },
    readText: async (file) => {
      read.push(file);
      return "# plan summary";
    },
    write: async (entry) => {
      written.push(entry);
      return {};
    },
    entries: async () => entries,
    dialogs,
    held,
  };
  return { written, cpd, read, broadcasted, stateChanges, port };
};

test("open writes the summary and the pending entry then asks the select dialog", async () => {
  const { written, read, broadcasted, port } = makePort([entryOf("r3", "pending")]);
  const pr = createPlanReview(port);
  await pr.open("r3", "event");
  assert.equal(read.length, 1);
  const summary = written.find((e) => e.kind === "cpd-plan-summary");
  assert.ok(summary, "the summary entry is written");
  assert.deepEqual(summary!.data, { plan: "p1", review: "r3", display: true });
  const entry = written.find((e) => e.kind === "cpd-plan-review");
  assert.ok(entry, "the pending entry is written");
  assert.deepEqual(entry!.data, { plan: "p1", review: "r3", surface: "chat", status: "pending" });
  const req = broadcasted.find((m) => m.type === "ui_request");
  assert.equal(req?.method, "select");
  assert.equal((req as { title: string }).title, "Approve the plan review");
  assert.deepEqual((req as { options: string[] }).options, ["Approve", "Change"]);
  assert.equal((req as { purpose: string }).purpose, "plan_review");
  assert.equal((req as { expires_at?: number }).expires_at, undefined);
});

test("open for a pending review via start reuses the pending entry and asks again with a fresh id", async () => {
  const { written, broadcasted, port } = makePort([entryOf("r3", "pending")]);
  const pr = createPlanReview(port);
  await pr.open("r3", "start");
  assert.equal(written.filter((e) => e.kind === "cpd-plan-review").length, 0);
  const reqs = broadcasted.filter((m) => m.type === "ui_request");
  assert.equal(reqs.length, 1);
  assert.equal((reqs[0] as { method: string }).method, "select");
});

test("Change opens the editor and decide runs cpd plan decide with the feedback file", async () => {
  const { written, cpd, port } = makePort([entryOf("r3", "pending")]);
  const decide = createPlanReview(port).decide;
  await decide({ plan: "p1", review: "r3", surface: "chat", status: "pending" }, false, "needs work");
  assert.equal(cpd.some((a) => a[0] === "plan" && a[1] === "decide"), true);
  assert.ok(cpd.some((a) => a.includes("--feedback-file")), "the feedback file flag is passed");
  assert.equal(written.some((e) => e.kind === "cpd-plan-decision"), true);
});

test("a cancelled select leaves the entry pending and sends a notice", async () => {
  const { written, broadcasted, port } = makePort([entryOf("r3", "pending")]);
  const pr = createPlanReview(port);
  await pr.open("r3", "event");
  assert.equal(written.filter((e) => e.kind === "cpd-plan-review" && (e.data.status as string) !== "pending").length, 0);
  const notice = broadcasted.find((m) => m.type === "notice") as { message: string } | undefined;
  assert.ok(notice, "a notice is sent");
  assert.match(notice!.message, /^Plan review r3 is still waiting/);
});

test("a newer review supersedes the older one closing its dialog and entry", async () => {
  const { broadcasted, port } = makePort([entryOf("r2", "pending"), entryOf("r3", "pending")]);
  const pr = createPlanReview(port);
  await pr.open("r2", "event", "# r2");
  await pr.open("r3", "event", "# r3");
  const resolved = broadcasted.filter((m) => m.type === "ui_resolved");
  const superseded = resolved.find((m) => (m as { outcome: string }).outcome === "superseded");
  assert.ok(superseded, "the older dialog is resolved as superseded");
  assert.equal((superseded as { by: string }).by, "host");
});

test("decidedElsewhere settles the pending entry without cpd plan decide", async () => {
  const { cpd, written, port } = makePort([entryOf("r3", "pending")]);
  const pr = createPlanReview(port);
  await pr.decidedElsewhere({ plan: "p1", review: "r3", surface: "chat", status: "pending" }, { kind: "plan-review", data: { review: "r3", status: "approved" } } as never);
  assert.equal(cpd.some((a) => a[0] === "plan" && a[1] === "decide"), false);
  const settled = written.find((e) => e.kind === "cpd-plan-review");
  assert.equal((settled?.data.status as string), "approved");
  assert.equal(written.some((e) => e.kind === "cpd-plan-decision"), true);
});

test("closeSuperseded closes the reason and records the entry", async () => {
  const { written, port } = makePort([entryOf("r3", "pending")]);
  const pr = createPlanReview(port);
  pr.closeSuperseded({ plan: "p1", review: "r3", surface: "chat", status: "pending" });
  const settled = written.find((e) => e.kind === "cpd-plan-review");
  assert.equal(settled?.data.status, "superseded");
});