import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";
import { Fixture, ev, planShow } from "./fake.ts";
import { reviewsFromEntries } from "../lib/plan-review.ts";

// hs-d738, hs-d739: plannotator is removed; the review is the chat dialog (Approve, Change) and its summary.

const TITLE = "Review title";
const SUMMARY = `# ${TITLE}\nPlan p1: Plan · review r1\n\nGoal: Show a review in chat\n\nApprove | Change\n`;
const pending = () => planShow({ review: { id: "r1", status: "pending", fingerprint: "ff", current: true, title: TITLE } });
const reviewEvent = (id: number) => ev(id, "plan-review", { data: { review: "r1", fingerprint: "ff", status: "pending", title: TITLE } });
const entries = (f: Fixture) => f.pi.entries.filter((e: any) => e.type === "custom" && e.customType === "cpd-plan-review").map((e: any) => e.data);

async function opens(f: Fixture) {
  f.respond("plan-show", pending());
  f.summaryFile("p1", "r1", SUMMARY);
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  f.emit(reviewEvent(101));
  await f.drain();
  const d = f.pi.openDialog("select");
  assert.ok(d, "the dialog is open");
  return d;
}

test("drop-plannotator the review dialog offers Approve and Change only", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  assert.deepEqual(d.options, ["Approve", "Change"]);
});

test("drop-plannotator a plan-review Event shows the summary and writes only chat entries", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const d = await opens(f);
  const summaries = f.pi.sent.filter((s) => s.message.customType === "cpd-plan-summary");
  assert.equal(summaries.length, 1);
  assert.equal(summaries[0]!.message.content, SUMMARY);
  assert.deepEqual(d.options, ["Approve", "Change"]);
  const written = entries(f);
  assert.ok(written.length > 0);
  for (const e of written) {
    assert.equal("plannotator_id" in e, false);
    assert.ok(e.surface === undefined || e.surface === "chat");
  }
  assert.deepEqual(f.pi.emitted.filter((e) => e.channel.startsWith("plannotator:")), []);
});

test("drop-plannotator an entry written for a plannotator page is read as a pending chat review", () => {
  const old = { type: "custom", customType: "cpd-plan-review", data: { plan: "p1", review: "r1", surface: "plannotator", plannotator_id: "x", status: "pending" } };
  const { reviews, invalid } = reviewsFromEntries([old], "p1");
  assert.equal(invalid, 0);
  assert.deepEqual(reviews.get("r1"), { plan: "p1", review: "r1", surface: "chat", status: "pending" });
});

test("drop-plannotator the adapter listens to no plannotator event and the plannotator module is gone", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  const names: string[] = [];
  const on = f.pi.events.on;
  f.pi.events.on = (channel: string, handler: (data: unknown) => void) => {
    names.push(channel);
    return on(channel, handler);
  };
  f.respond("plan-show", pending());
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  assert.deepEqual(names.filter((n) => n.startsWith("plannotator:")), []);
  assert.equal(existsSync(new URL("../lib/plannotator.ts", import.meta.url)), false);
});
