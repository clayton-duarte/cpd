import assert from "node:assert/strict";
import { test } from "node:test";
import { reviewsFromEntries } from "../lib/plan-review.ts";
import { createPlanReview } from "../durable-host/plan-review.ts";
import { createHeld } from "../durable-host/held.ts";
import { createDialogs } from "../durable-host/dialogs.ts";

test("durable review records reconcile pending and externally decided reviews", () => {
  const record = (status: string) => ({
    kind: "cpd-plan-review", model: [],
    data: { plan: "p1", review: "r1", surface: "chat", status },
  });
  const pending = reviewsFromEntries([record("pending")], "p1");
  assert.equal(pending.reviews.get("r1")?.status, "pending");
  const settled = reviewsFromEntries([record("pending"), record("approved")], "p1");
  assert.equal(settled.reviews.get("r1")?.status, "approved");
  assert.equal(settled.invalid, 0);
});

test("mixed legacy and durable reviews preserve ordering and Plan isolation", () => {
  const data = { plan: "p1", review: "r1", surface: "chat", status: "pending" };
  const result = reviewsFromEntries([
    { type: "custom", customType: "cpd-plan-review", data },
    { kind: "cpd-plan-review", data: { ...data, status: "rejected" }, model: [] },
    { kind: "cpd-plan-review", data: { ...data, plan: "p2", status: "approved" }, model: [] },
    { kind: "cpd-plan-summary", data: { ...data, status: "approved" }, model: [] },
    { kind: "cpd-plan-review", data: { ...data, status: "invalid" }, model: [] },
  ], "p1");
  assert.equal(result.reviews.get("r1")?.status, "rejected");
  assert.equal(result.reviews.size, 1);
  assert.equal(result.invalid, 1);
});

test("an external approval closes the live dialog backed by durable records", async () => {
  const held = createHeld();
  const records: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }[] = [];
  const calls: readonly string[][] = [];
  const dialogs = createDialogs({ held, broadcast: () => {}, stateChanged: () => {} });
  const flow = createPlanReview({
    plan: "p1", held, dialogs,
    entries: async () => records,
    write: async (record) => { records.push(record); },
    readText: async () => "# Review r1",
    cpd: async (args) => {
      (calls as string[][]).push([...args]);
      return { review: { id: "r1", status: "approved", current: true } };
    },
  });
  await flow.open("r1", "event");
  assert.equal(held.heldCount(), 1);
  const pending = reviewsFromEntries(records, "p1").reviews.get("r1");
  assert.ok(pending, "the durable pending record must be visible to the event handler");
  await flow.decidedElsewhere(pending, {
    kind: "plan-review", data: { review: "r1", status: "approved" },
  } as never);
  assert.equal(held.heldCount(), 0);
  assert.equal(reviewsFromEntries(records, "p1").reviews.get("r1")?.status, "approved");
  assert.equal(calls.some(args => args[0] === "plan" && args[1] === "decide"), false,
    "an external approval must settle the dialog without making another decision");
});

test("restart reconciles an already-approved review instead of reopening its dialog", async () => {
  const held = createHeld();
  const records = [{ kind: "cpd-plan-review", model: [] as readonly unknown[],
    data: { plan: "p1", review: "r1", surface: "chat", status: "pending" } as Record<string, unknown> }];
  const flow = createPlanReview({
    plan: "p1", held,
    dialogs: createDialogs({ held, broadcast: () => {}, stateChanged: () => {} }),
    entries: async () => records,
    write: async (record) => { records.push(record); },
    readText: async () => { throw new Error("a settled review needs no summary file"); },
    cpd: async () => ({ review: { id: "r1", status: "approved", current: true } }),
  });
  await flow.open("r1", "start");
  assert.equal(held.heldCount(), 0);
  assert.equal(reviewsFromEntries(records, "p1").reviews.get("r1")?.status, "approved");
});
