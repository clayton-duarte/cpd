import assert from "node:assert/strict";
import { test, type TestContext } from "node:test";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §10.18 and §11, contract §5.2 (Plan agent-refusals): an adapter guard
// refusal is reported through the `cpd session report` path as a `guard` Event, with
// the guard rule as its code and the refused command as its excerpt (§11's worked
// example: a Worker's `explore network write` of a `git fetch`).

/** The guard rule of `φ: explore network write: …` — the refusal text's prefix (§5.2). */
const GUARD = "explore network write";

/** The refused command line: the excerpt the report carries (§5.2). */
const COMMAND = "git fetch origin agent-refusals/miner";

/** A started adapter whose Session is a scout (CPD_JOB_KIND=scout), so the explore profile applies. */
async function started(t: TestContext): Promise<Fixture> {
  const saved = process.env.CPD_JOB_KIND;
  t.after(() => {
    if (saved === undefined) delete process.env.CPD_JOB_KIND;
    else process.env.CPD_JOB_KIND = saved;
  });
  process.env.CPD_JOB_KIND = "scout";
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "worker", plan: "p1", plan_lead: false }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  return f;
}

test("a blocked guard call reports a guard Event", async (t) => {
  const f = await started(t);
  const verdict = (
    await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command: COMMAND, timeout: 10 } })
  )[0] as { block?: boolean; reason?: string } | undefined;
  assert.equal(verdict?.block, true, "the explore guard refuses the scout's git fetch");

  // The adapter reports the refusal through `cpd session report`; the fake cpd records the argv.
  const report = await waitFor("the guard report", () =>
    f.reports().find((r) => JSON.stringify(r).includes(GUARD)),
  );
  const blob = JSON.stringify(report);
  assert.ok(blob.includes(COMMAND), `the report carries the refused command's excerpt: ${blob}`);
});
