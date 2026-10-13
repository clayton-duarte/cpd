import { test } from "node:test";
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { latestApprovedFingerprint, lockPlan } from "../../src/ledger/plan.ts";
import { tmpDir } from "../helpers/index.ts";
import { addDraft, cliSandbox, bdJson } from "../ledger/helpers.ts";

const { repo, cli } = cliSandbox();
const dir = tmpDir();
const wf = "pdcli";

test("plan decision records pending, then approved with feedback; plan lock then needs nothing else", async () => {
  await cli(["workflow", "init", "--wf", wf, "--repo", repo, "--title", "PD"]);
  await cli(["workflow", "phase", "plan", "--wf", wf]);
  await addDraft(wf, "a");

  const pending = await cli(["plan", "decision", "--wf", wf, "--review-id", "rev-1", "--pending"]);
  assert.equal(pending.code, 0, pending.stdout + pending.stderr);
  assert.deepEqual([pending.json.ok, pending.json.reviewId, pending.json.status], [true, "rev-1", "pending"]);
  assert.match(pending.json.fingerprint, /^[0-9a-f]{64}$/);

  const feedback = join(dir, "fb.md");
  writeFileSync(feedback, "looks good\n");
  const approved = await cli(["plan", "decision", "--wf", wf, "--review-id", "rev-1", "--approved", "--feedback-file", feedback]);
  assert.equal(approved.code, 0, approved.stdout);
  assert.equal(approved.json.status, "approved");
  assert.equal(approved.json.fingerprint, pending.json.fingerprint);
  assert.equal(await latestApprovedFingerprint(wf), pending.json.fingerprint);

  const issues: any[] = await bdJson(wf, ["list", "--all", "-n", "0", "-t", "workflow"]);
  const rec = issues[0].metadata["plan_review_rev-1"];
  assert.deepEqual([rec.status, rec.approved, rec.feedback, rec.fingerprint], ["approved", true, "looks good\n", pending.json.fingerprint]);

  const lock = await lockPlan(wf);
  assert.equal(lock.fingerprint, pending.json.fingerprint);
});

test("plan decision refuses a repeated state, and bad usage", async () => {
  const again = await cli(["plan", "decision", "--wf", wf, "--review-id", "rev-1", "--approved"]);
  assert.equal(again.code, 3);
  assert.equal(again.json.error.code, "illegal_phase", "phase is dispatch after the lock");

  const wf2 = "pdcli2";
  await cli(["workflow", "init", "--wf", wf2, "--repo", repo, "--title", "PD2"]);
  await cli(["workflow", "phase", "plan", "--wf", wf2]);
  assert.equal((await cli(["plan", "decision", "--wf", wf2, "--review-id", "r", "--rejected", "--feedback-file", join(dir, "fb.md")])).code, 0);
  const twice = await cli(["plan", "decision", "--wf", wf2, "--review-id", "r", "--rejected"]);
  assert.equal(twice.code, 3);
  assert.equal(twice.json.error.code, "decision_recorded");

  for (const args of [["--pending", "--approved"], [], ["--pending", "--feedback-file", "/nonexistent"]]) {
    const r = await cli(["plan", "decision", "--wf", wf2, "--review-id", "q", ...args]);
    assert.equal(r.code, 2, args.join(" "));
  }
  assert.equal((await cli(["plan", "decision", "--wf", wf2, "--pending"])).code, 2, "review id required");
  assert.equal((await cli(["plan", "decision", "--wf", wf2, "--review-id", "bad id", "--pending"])).code, 2);
});
