import { test } from "node:test";
import assert from "node:assert/strict";
import { addJob, cancelJob, getJobByKey, listJobs, readyJobs } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { addDraft, approvePlan, CATALOG, mergeJob, newPlan, sandbox, spec } from "./helpers.ts";

const { repo } = sandbox();
const keys = async () => (await readyJobs("graph")).map((j) => j.key);

// t2 F3: A; B and C need A; D needs B and C; E independent.
test("readyJobs follows the t2 F3 graph, including Cancelled keeping its dependents blocked", async () => {
  await newPlan("graph", repo);
  await addDraft("graph", "a");
  await addDraft("graph", "b", ["a"]);
  await addDraft("graph", "c", ["a"]);
  await addDraft("graph", "d", ["b", "c"]);
  await addDraft("graph", "e");
  assert.deepEqual(await keys(), [], "drafts are never ready");

  await approvePlan("graph");
  await lockPlan("graph");
  assert.deepEqual(await keys(), ["a", "e"]);
  assert.deepEqual((await listJobs("graph")).map((j) => j.status), ["queued", "queued", "queued", "queued", "queued"]);

  await mergeJob("graph", "a");
  assert.equal((await getJobByKey("graph", "a")).status, "closed");
  assert.deepEqual(await keys(), ["b", "c", "e"]);

  await mergeJob("graph", "b");
  assert.deepEqual(await keys(), ["c", "e"]);

  const r = await cancelJob("graph", "c", "descoped");
  assert.equal(r.cancelled, true);
  assert.deepEqual(await keys(), ["e"], "D stays blocked behind the cancelled C");

  // F appended in phase dispatch: queued at once, needs B (merged), so ready.
  const f = await addJob("graph", { key: "f", title: "F", tier: "S2", reviewTier: "S4", spec: spec(), needs: ["b"] }, CATALOG);
  assert.equal(f.job.status, "queued");
  assert.match(f.fingerprint ?? "", /^[0-9a-f]{64}$/);
  assert.deepEqual(await keys(), ["e", "f"]);
  assert.equal((await getJobByKey("graph", "d")).status, "queued");
});

test("a Job appended after lock cannot need a cancelled Job", async () => {
  await assert.rejects(
    addJob("graph", { key: "h", title: "H", tier: "S2", reviewTier: "S4", spec: spec(), needs: ["c"] }, CATALOG),
    (e: any) => e.code === "needs_unavailable" && e.exitCode === 3,
  );
});
