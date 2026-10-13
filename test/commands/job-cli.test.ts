import { test } from "node:test";
import assert from "node:assert/strict";
import { lockPlan, recordPlanDecision } from "../../src/ledger/plan.ts";
import { tmpDir } from "../helpers/index.ts";
import { allIssues, cliSandbox, makeAttempt, setJobStatus, spec, writeSpec } from "../ledger/helpers.ts";

const { repo, cli } = cliSandbox();
const dir = tmpDir();
const wf = "jobcli";
const specFile = writeSpec(dir, "spec.json", spec());
const add = (key: string, ...more: string[]) => cli(["job", "add", "--wf", wf, "--key", key, "--title", `Job ${key}`, "--tier", "S3", "--spec", specFile, ...more]);

test("setup: Workflow in phase plan", async () => {
  assert.equal((await cli(["workflow", "init", "--wf", wf, "--repo", repo, "--title", "Job CLI"])).code, 0);
  assert.equal((await cli(["workflow", "phase", "plan", "--wf", wf])).code, 0);
});

test("job add creates a draft with the default review tier; list and show print it", async () => {
  const r = await add("api");
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.json.ok, true);
  assert.equal(r.json.job.key, "api");
  assert.equal(r.json.job.status, "draft");
  assert.equal(r.json.job.reviewTier, "S4");
  assert.equal(r.json.fingerprint, null);
  const b = await add("web", "--needs", "api", "--review-tier", "O3");
  assert.equal(b.code, 0, b.stdout);
  assert.deepEqual([b.json.job.needs, b.json.job.reviewTier], [["api"], "O3"]);

  const list = await cli(["job", "list", "--wf", wf]);
  assert.deepEqual(list.json.jobs.map((j: any) => [j.key, j.status, j.needs]), [["api", "draft", []], ["web", "draft", ["api"]]]);
  assert.equal((await cli(["job", "list", "--wf", wf, "--status", "queued"])).json.jobs.length, 0);
  assert.equal((await cli(["job", "list", "--wf", wf, "--status", "bogus"])).code, 2);

  const show = await cli(["job", "show", "api", "--wf", wf]);
  assert.equal(show.code, 0, show.stdout);
  assert.equal(show.json.job.goal, "do the thing");
  assert.deepEqual(show.json.job.touches, ["src/a.ts"]);
  assert.deepEqual([show.json.attempts, show.json.comments], [[], []]);
  const missing = await cli(["job", "show", "ghost", "--wf", wf]);
  assert.equal(missing.code, 4);
  assert.equal(missing.json.error.code, "job_not_found");
});

test("invalid job add inputs are refused with specific codes and exit statuses; nothing is written", async () => {
  const before = await allIssues(wf);
  const emptyTouches = writeSpec(dir, "empty.json", { ...spec(), touches: [] });
  const garbage = writeSpec(dir, "garbage.json", "{not json");
  const cases: [string[], string, number][] = [
    [["--key", "Bad_Key", "--title", "t", "--tier", "S3", "--spec", specFile], "invalid_job_key", 2],
    [["--key", "k1", "--title", "t", "--tier", "S9", "--spec", specFile], "invalid_tier", 2],
    [["--key", "k1", "--title", "t", "--tier", "O0", "--spec", specFile], "tier_invalid", 2],
    [["--key", "k1", "--title", "t", "--tier", "S3", "--spec", specFile, "--needs", "ghost"], "needs_unknown", 2],
    [["--key", "k1", "--title", "t", "--tier", "S3", "--spec", specFile, "--needs", "k1"], "dependency_cycle", 3],
    [["--key", "k1", "--title", "t", "--tier", "S3", "--spec", emptyTouches], "spec_invalid", 2],
    [["--key", "k1", "--title", "t", "--tier", "S3", "--spec", garbage], "spec_invalid", 2],
    [["--key", "k1", "--title", "t", "--tier", "S3", "--spec", "/nonexistent.json"], "spec_unreadable", 2],
    [["--key", "api", "--title", "t", "--tier", "S3", "--spec", specFile], "job_key_exists", 3],
    [["--key", "k1", "--title", "t", "--tier", "S3"], "usage", 2],
    [["--key", "k1", "--title", "t", "--tier", "S3", "--spec", specFile, "--bogus", "1"], "usage", 2],
  ];
  for (const [args, code, exit] of cases) {
    const r = await cli(["job", "add", "--wf", wf, ...args]);
    assert.equal(r.json.error?.code, code, args.join(" "));
    assert.equal(r.code, exit, args.join(" "));
  }
  assert.deepEqual(await allIssues(wf), before);
});

test("job edit changes a draft; a cycle is refused", async () => {
  const other = writeSpec(dir, "other.json", spec({ goal: "other goal" }));
  const r = await cli(["job", "edit", "api", "--wf", wf, "--title", "API v2", "--tier", "H2", "--spec", other, "--review-tier", "S5"]);
  assert.equal(r.code, 0, r.stdout);
  assert.deepEqual([r.json.job.title, r.json.job.tier, r.json.job.reviewTier], ["API v2", "H2", "S5"]);
  assert.equal((await cli(["job", "show", "api", "--wf", wf])).json.job.goal, "other goal");
  const cycle = await cli(["job", "edit", "api", "--wf", wf, "--needs", "web"]);
  assert.equal(cycle.code, 3);
  assert.equal(cycle.json.error.code, "dependency_cycle");
  const none = await cli(["job", "edit", "api", "--wf", wf]);
  assert.equal(none.code, 2);
});

test("after lock: job edit is job_locked (exit 3); add appends a queued, fingerprinted Job", async () => {
  await recordPlanDecision(wf, "r1", "pending");
  await recordPlanDecision(wf, "r1", "approved");
  await lockPlan(wf);
  const edit = await cli(["job", "edit", "api", "--wf", wf, "--title", "nope"]);
  assert.equal(edit.code, 3);
  assert.equal(edit.json.error.code, "job_locked");
  const appended = await add("late", "--needs", "api");
  assert.equal(appended.code, 0, appended.stdout);
  assert.equal(appended.json.job.status, "queued");
  assert.match(appended.json.fingerprint, /^[0-9a-f]{64}$/);
});

test("job cancel: no running Attempt -> cancelled; running Attempt -> cancel_requested; closed/cancelled refused", async () => {
  const noReason = await cli(["job", "cancel", "late", "--wf", wf]);
  assert.equal(noReason.code, 2);
  const r = await cli(["job", "cancel", "late", "--wf", wf, "--reason", "not needed"]);
  assert.equal(r.code, 0, r.stdout);
  assert.equal(r.json.job.status, "cancelled");
  assert.equal(r.json.cancelled, true);
  const again = await cli(["job", "cancel", "late", "--wf", wf, "--reason", "x"]);
  assert.equal(again.code, 3);
  assert.equal(again.json.error.code, "job_not_cancellable");

  await setJobStatus(wf, "web", "running");
  await makeAttempt(wf, "web", 1, "in_progress");
  const req = await cli(["job", "cancel", "web", "--wf", wf, "--reason", "stop"]);
  assert.equal(req.code, 0, req.stdout);
  assert.deepEqual([req.json.job.status, req.json.cancelled, req.json.cancelRequested], ["running", false, true]);
});

test("job retry: refused unless failed; on a failed Job it records the Tier and queues", async () => {
  const notFailed = await cli(["job", "retry", "api", "--wf", wf, "--tier", "S4"]);
  assert.equal(notFailed.code, 3);
  assert.equal(notFailed.json.error.code, "job_not_failed");
  assert.equal((await cli(["job", "retry", "api", "--wf", wf])).code, 2);
  await setJobStatus(wf, "api", "failed");
  const bad = await cli(["job", "retry", "api", "--wf", wf, "--tier", "S9"]);
  assert.equal(bad.code, 2);
  const ok = await cli(["job", "retry", "api", "--wf", wf, "--tier", "O4"]);
  assert.equal(ok.code, 0, ok.stdout);
  assert.deepEqual([ok.json.job.status, ok.json.attempt, ok.json.tier], ["queued", 1, "O4"]);
  assert.equal((await cli(["job", "show", "api", "--wf", wf])).json.job.metadata.retry_1_tier, "O4");
});
