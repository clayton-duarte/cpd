import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { startAttempt } from "../../src/ledger/attempts.ts";
import { readFingerprints } from "../../src/ledger/fingerprint.ts";
import { addJob, getJobByKey, listJobs } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { getPhase } from "../../src/ledger/workflow.ts";
import { cpdLogPath } from "../../src/lib/paths.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { fakeBin, tmpDir } from "../helpers/index.ts";
import { addDraft, approvePlan, CATALOG, newPlan, sandbox, spec } from "./helpers.ts";

const { repo } = sandbox();

const crash = (at: string) => (step: string) => {
  if (step === at) throw new Error(`injected crash after ${step}`);
};

for (const step of ["fingerprints", "queue"] as const) {
  test(`lockPlan: a crash after ${step} is finished by a re-run`, async () => {
    const wf = `lock-${step}`;
    await newPlan(wf, repo);
    await addDraft(wf, "a");
    await addDraft(wf, "b", ["a"]);
    await approvePlan(wf);
    await assert.rejects(lockPlan(wf, crash(step)), /injected crash/);
    assert.equal(await getPhase(wf), "plan");
    const stored = readFingerprints(wf);
    const r = await lockPlan(wf);
    assert.deepEqual(r.jobs, ["a", "b"]);
    assert.equal(await getPhase(wf), "dispatch");
    assert.deepEqual((await listJobs(wf)).map((j) => j.status), ["queued", "queued"]);
    const after = readFingerprints(wf);
    assert.deepEqual(Object.keys(after).sort(), ["a", "b"]);
    if (step === "queue") assert.deepEqual(after, stored, "the resume keeps the stored fingerprints");
  });
}

test("lockPlan resume: a different stored fingerprint is still fingerprint_exists", async () => {
  const wf = "lock-differs";
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await approvePlan(wf);
  await assert.rejects(lockPlan(wf, crash("fingerprints")), /injected crash/);
  // the draft changes after its fingerprint was stored
  const { editJob } = await import("../../src/ledger/jobs.ts");
  await editJob(wf, "a", { spec: spec({ goal: "changed" }) }, CATALOG);
  await approvePlan(wf, "r2");
  await assert.rejects(lockPlan(wf), (e) => e instanceof CpdError && e.code === "fingerprint_exists");
  assert.equal(await getPhase(wf), "plan");
});

test("dispatch-phase addJob: a crash after the fingerprint is finished by a re-run with the same input", async () => {
  const wf = "add-crash";
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await approvePlan(wf);
  await lockPlan(wf);
  const input = { key: "late", title: "Late", tier: "S3", reviewTier: "S4", spec: spec({ goal: "late goal" }), needs: ["a"] };
  await assert.rejects(addJob(wf, input, CATALOG, crash("fingerprint")), /injected crash/);
  assert.equal((await getJobByKey(wf, "late")).status, "draft");
  const stored = readFingerprints(wf).late;
  assert.ok(stored);

  // other input for the same key is still refused
  await assert.rejects(addJob(wf, { ...input, title: "x", spec: spec({ goal: "other" }) }, CATALOG), (e) => e instanceof CpdError && e.code === "job_key_exists");
  const r = await addJob(wf, input, CATALOG);
  assert.equal(r.job.status, "queued");
  assert.equal(r.fingerprint, stored);
  assert.deepEqual(readFingerprints(wf).late, stored);
});

test("startAttempt: a failed rollback is logged and attached to the error", async () => {
  const wf = "rollback";
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await approvePlan(wf);
  await lockPlan(wf);
  // bd that fails `create` and the rollback `update … -s queued`, and runs the real bd otherwise
  const real = (await import("node:child_process")).execFileSync("which", ["bd"], { encoding: "utf8" }).trim();
  const bin = fakeBin("bd", `case "$1" in create) echo '{"error":"boom create"}'; exit 1;; esac
case " $* " in *" -s queued "*) echo '{"error":"boom rollback"}'; exit 1;; esac
exec '${real}' "$@"`, tmpDir("cpd-bin-"));
  const oldPath = process.env.PATH;
  process.env.PATH = `${bin.dir}:${oldPath}`;
  try {
    await assert.rejects(startAttempt(wf, "a", { tier: "S3" }), (e: unknown) => {
      assert.ok(e instanceof CpdError);
      assert.match(String(e.details.rollbackError), /boom rollback/);
      assert.match(e.message, /boom create/);
      return true;
    });
  } finally {
    process.env.PATH = oldPath;
  }
  const events = readFileSync(cpdLogPath(wf), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
  const ev = events.find((e) => e.event === "attempt_start_rollback_failed");
  assert.equal(ev?.level, "error");
  assert.match(ev.rollbackError, /boom rollback/);
});
