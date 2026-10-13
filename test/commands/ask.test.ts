import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { runCli } from "../helpers/index.ts";
import { listOpenQuestions } from "../../src/queue/escalation-files.ts";
import { setup, WF } from "../queue/helpers.ts";

const msgs = (dir: string) =>
  readdirSync(dir).filter((n) => n.endsWith(".json")).sort().map((n) => JSON.parse(readFileSync(join(dir, n), "utf8")));

async function until<T>(f: () => T | undefined | false, ms = 10000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const v = f();
    if (v) return v;
    if (Date.now() > end) throw new Error("until: timed out");
    await sleep(50);
  }
}

test("ask blocks; answer from another process unblocks it; files and messages are ordered", async () => {
  const { env, queueDir, escDir } = setup();
  const asking = runCli(["ask", "job-a1", "which db?", "--wf", WF], { env });
  await until(() => existsSync(join(escDir, "job-a1-q1.q.json")) && msgs(queueDir).length === 1);
  const esc = msgs(queueDir)[0];
  assert.equal(esc.kind, "escalation");
  assert.equal(esc.attempt, "job-a1");
  assert.ok(esc.text.endsWith(`cpd answer job-a1-q1 "<answer>" --wf ${WF}`), esc.text);
  const q = JSON.parse(readFileSync(join(escDir, "job-a1-q1.q.json"), "utf8"));
  assert.deepEqual(Object.keys(q).sort(), ["askedAt", "askerPid", "attempt", "id", "question", "v", "wf"]);
  await sleep(1200); // still blocked
  assert.equal(existsSync(join(escDir, "job-a1-q1.a.json")), false);

  const ans = await runCli(["answer", "job-a1-q1", "postgres", "--wf", WF], { env });
  assert.equal(ans.code, 0, ans.stdout + ans.stderr);
  const done = await asking;
  assert.equal(done.code, 0, done.stderr);
  assert.equal(done.json.ok, true);
  assert.equal(done.json.answer, "postgres");
  assert.deepEqual(msgs(queueDir).map((m) => m.kind), ["escalation", "escalation-answered"]);
  const a = JSON.parse(readFileSync(join(escDir, "job-a1-q1.a.json"), "utf8"));
  assert.deepEqual(Object.keys(a).sort(), ["answer", "answeredAt", "id", "v"]);
});

test("Lead-restart durability: a question with no reader is listed; answering later unblocks the asker", async () => {
  const { env, escDir, queueDir } = setup();
  const asking = runCli(["ask", "job-a1", "q?", "--wf", WF], { env });
  await until(() => existsSync(join(escDir, "job-a1-q1.q.json")));
  // "Lead restarts": nobody consumed the queue; the question is still discoverable from files alone.
  await sleep(1000);
  assert.deepEqual(listOpenQuestions(WF).map((q) => q.id), ["job-a1-q1"]);
  assert.equal(msgs(queueDir).length, 1);
  const ans = await runCli(["answer", "job-a1-q1", "late", "--wf", WF], { env });
  assert.equal(ans.code, 0);
  assert.equal((await asking).json.answer, "late");
});

test("timeout exits 5, renames to .expired.json (id never reused); later answer is escalation_missing; double answer is escalation_answered", async () => {
  const { env, escDir } = setup();
  const t0 = Date.now();
  const r = await runCli(["ask", "job-a1", "q?", "--timeout-minutes", "0.05", "--wf", WF], { env });
  assert.equal(r.code, 5);
  assert.equal(r.json.error.code, "ask_timeout");
  assert.ok(Date.now() - t0 >= 2900);
  assert.deepEqual(readdirSync(escDir).filter((n) => n.endsWith(".q.json")), []);
  assert.deepEqual(readdirSync(escDir).filter((n) => n.endsWith(".expired.json")), ["job-a1-q1.expired.json"]);
  assert.deepEqual(listOpenQuestions(WF), []);
  const late = await runCli(["answer", "job-a1-q1", "x", "--wf", WF], { env });
  assert.equal(late.code, 3);
  assert.equal(late.json.error.code, "escalation_missing");

  const asking = runCli(["ask", "job-a1", "again?", "--wf", WF], { env });
  await until(() => existsSync(join(escDir, "job-a1-q2.q.json")));
  assert.equal(existsSync(join(escDir, "job-a1-q1.expired.json")), true);
  assert.equal((await runCli(["answer", "job-a1-q2", "one", "--wf", WF], { env })).code, 0);
  await asking;
  const twice = await runCli(["answer", "job-a1-q2", "two", "--wf", WF], { env });
  assert.equal(twice.code, 3);
  assert.equal(twice.json.error.code, "escalation_answered");
  assert.equal((await runCli(["answer", "job-a1-q1", "x", "--wf", WF], { env })).json.error.code, "escalation_missing");
});

test("ask without an attached Lead fails lead_not_attached and leaves no question", async () => {
  const { env, escDir } = setup(false);
  const r = await runCli(["ask", "job-a1", "q?", "--wf", WF], { env });
  assert.equal(r.code, 3);
  assert.equal(r.json.error.code, "lead_not_attached");
  assert.equal(existsSync(escDir) ? readdirSync(escDir).length : 0, 0);
});

test("ask usage errors", async () => {
  const { env } = setup();
  assert.equal((await runCli(["ask", "job-a1", "q"], { env })).json.error.code, "workflow_required");
  assert.equal((await runCli(["ask", "job-a1", "q", "--bogus", "--wf", WF], { env })).code, 2);
  assert.equal((await runCli(["ask", "nope", "q", "--wf", WF], { env })).code, 2);
});
