import { test } from "node:test";
import assert from "node:assert/strict";
import { CpdError } from "../../src/lib/errors.ts";
import { failAttempt, getAttempt, markMerged, markSubmitted, recordVerdict, startAttempt } from "../../src/ledger/attempts.ts";
import { expireEscalation, openEscalations, recordAnswer, recordEscalation } from "../../src/ledger/escalations.ts";
import { getJobByKey } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, allIssues, approvePlan, bdJson, newPlan, sandbox } from "./helpers.ts";

const { repo } = sandbox();
const wf = "esc";
const refusal = (code: string, exit = 3) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;
const sha = (c: string) => c.repeat(40);

test("5. record, duplicate is a no-op, listed as open, closing the Attempt is escalation_open, answer, close succeeds", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await addDraft(wf, "b");
  await approvePlan(wf);
  await lockPlan(wf);
  await startAttempt(wf, "a", { tier: "S3" });
  await startAttempt(wf, "b", { tier: "S3" });

  const q = { escId: "a-a1-q1", question: "--which schema?", askedAt: "2026-01-01T00:00:00.000Z" };
  const first = await recordEscalation(wf, "a-a1", q);
  assert.equal(first.created, true);
  assert.deepEqual([first.escalation.id, first.escalation.attempt, first.escalation.question, first.escalation.status], ["a-a1-q1", "a-a1", "--which schema?", "open"]);
  const before = await allIssues(wf);
  const dup = await recordEscalation(wf, "a-a1", { ...q, question: "different text" });
  assert.equal(dup.created, false);
  assert.equal(dup.escalation.question, "--which schema?");
  assert.deepEqual(await allIssues(wf), before, "a duplicate record writes nothing");

  assert.deepEqual((await openEscalations(wf)).map((e) => [e.id, e.attempt, e.askedAt]), [["a-a1-q1", "a-a1", q.askedAt]]);

  // the Attempt cannot be closed while the Escalation is open; nothing is written
  await assert.rejects(failAttempt(wf, "a-a1", "timed_out", { requeue: true }), refusal("escalation_open"));
  assert.deepEqual(await allIssues(wf), before);
  assert.equal((await getAttempt(wf, "a-a1")).status, "in_progress");
  assert.equal((await getJobByKey(wf, "a")).status, "running");

  // same for the merge path
  await markSubmitted(wf, "a-a1", sha("a"));
  await recordVerdict(wf, "a-a1", { verdict: "approve", feedback: "ok" });
  await assert.rejects(markMerged(wf, "a-a1", sha("b")), refusal("escalation_open"));

  const answered = await recordAnswer(wf, "a-a1-q1", { answer: "use v2", answeredAt: "2026-01-01T00:05:00.000Z" });
  assert.equal(answered.recorded, true);
  assert.deepEqual([answered.escalation.status, answered.escalation.answeredBy, answered.escalation.answeredAt], ["closed", "lead", "2026-01-01T00:05:00.000Z"]);
  assert.deepEqual(await openEscalations(wf), []);
  const [esc] = await bdJson(wf, ["list", "--all", "-n", "0", "-t", "escalation"]);
  assert.deepEqual((await bdJson(wf, ["comments", esc.id])).map((c: any) => c.text), ["use v2"]);
  assert.equal(esc.close_reason, "answered");

  // answering again is a no-op, and the Attempt can now be closed
  const again = await recordAnswer(wf, "a-a1-q1", { answer: "other", answeredAt: "2026-01-01T00:06:00.000Z" });
  assert.equal(again.recorded, false);
  assert.deepEqual((await bdJson(wf, ["comments", esc.id])).length, 1);
  const merged = await markMerged(wf, "a-a1", sha("b"));
  assert.equal(merged.status, "closed");
});

test("Escalation errors: unknown id, wrong Attempt, closed Attempt, bad input", async () => {
  await assert.rejects(recordAnswer(wf, "a-a1-q9", { answer: "x", answeredAt: "t" }), refusal("escalation_not_found", 4));
  await assert.rejects(recordEscalation(wf, "b-a1", { escId: "a-a1-q2", question: "q", askedAt: "t" }), refusal("invalid_escalation_id", 2));
  await assert.rejects(recordEscalation(wf, "b-a1", { escId: "nope", question: "q", askedAt: "t" }), refusal("invalid_escalation_id", 2));
  await assert.rejects(recordEscalation(wf, "b-a1", { escId: "b-a1-q1", question: "", askedAt: "t" }), refusal("invalid_escalation", 2));
  await assert.rejects(recordAnswer(wf, "b-a1-q1", { answer: "", answeredAt: "t" }), refusal("invalid_answer", 2));
  await failAttempt(wf, "b-a1", "crashed", { requeue: true });
  await assert.rejects(recordEscalation(wf, "b-a1", { escId: "b-a1-q1", question: "q", askedAt: "t" }), refusal("illegal_transition"));
  assert.deepEqual(await openEscalations(wf), []);
});

test("expire: unblocks failAttempt and markMerged, idempotent, refuses an answered Escalation", async () => {
  const w = "esc2";
  await newPlan(w, repo);
  await addDraft(w, "c");
  await addDraft(w, "d");
  await approvePlan(w);
  await lockPlan(w);
  await startAttempt(w, "c", { tier: "S3" });
  await startAttempt(w, "d", { tier: "S3" });
  const at = "2026-01-01T00:00:00.000Z";
  await recordEscalation(w, "c-a1", { escId: "c-a1-q1", question: "q?", askedAt: at });
  await recordEscalation(w, "d-a1", { escId: "d-a1-q1", question: "q?", askedAt: at });
  await assert.rejects(failAttempt(w, "c-a1", "timed_out", { requeue: true }), refusal("escalation_open"));

  const r = await expireEscalation(w, "c-a1-q1", { reason: "timeout", expiredAt: "2026-01-01T01:00:00.000Z" });
  assert.equal(r.recorded, true);
  assert.equal(r.escalation.status, "closed");
  const esc = (await bdJson(w, ["list", "--all", "-n", "0", "-t", "escalation"])).find((e: any) => e.metadata.esc_id === "c-a1-q1");
  assert.equal(esc.close_reason, "expired");
  assert.deepEqual([esc.metadata.expired_at, esc.metadata.expired_reason], ["2026-01-01T01:00:00.000Z", "timeout"]);
  assert.deepEqual((await bdJson(w, ["comments", esc.id])).map((c: any) => c.text), ["expired: timeout"]);
  assert.deepEqual((await openEscalations(w)).map((e) => e.id), ["d-a1-q1"]);

  const before = await allIssues(w);
  assert.equal((await expireEscalation(w, "c-a1-q1", { reason: "orphan", expiredAt: "later" })).recorded, false);
  assert.deepEqual(await allIssues(w), before, "a second expire writes nothing");

  assert.equal((await failAttempt(w, "c-a1", "timed_out", { requeue: true })).status, "closed");

  // merge path
  await expireEscalation(w, "d-a1-q1", { reason: "orphan", expiredAt: at });
  await markSubmitted(w, "d-a1", sha("a"));
  await recordVerdict(w, "d-a1", { verdict: "approve", feedback: "ok" });
  assert.equal((await markMerged(w, "d-a1", sha("b"))).status, "closed");

  // answered Escalation is refused
  await startAttempt(w, "c", { tier: "S3" });
  await recordEscalation(w, "c-a2", { escId: "c-a2-q1", question: "q?", askedAt: at }).catch(() => {});
  await recordAnswer(w, "c-a2-q1", { answer: "yes", answeredAt: at });
  await assert.rejects(expireEscalation(w, "c-a2-q1", { reason: "timeout", expiredAt: at }), refusal("escalation_answered"));
  await assert.rejects(expireEscalation(w, "c-a9-q1", { reason: "timeout", expiredAt: at }), refusal("escalation_not_found", 4));
});
