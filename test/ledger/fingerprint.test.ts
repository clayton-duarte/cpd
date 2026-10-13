import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { listTamper } from "../../src/ledger/audit.ts";
import { canonicalJson, fingerprint, readFingerprints, verifyJob } from "../../src/ledger/fingerprint.ts";
import { addJob, cancelJob } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { fingerprintsPath } from "../../src/lib/paths.ts";
import { addDraft, approvePlan, bdJson, CATALOG, jobId, newPlan, rawBd, sandbox, spec } from "./helpers.ts";

const { repo } = sandbox();
const wf = "fp";

const refusal = (code: string, exit: number) => (e: unknown) => e instanceof CpdError && e.code === code && e.exitCode === exit;

test("canonicalJson sorts keys recursively and is whitespace-free", () => {
  assert.equal(canonicalJson({ b: 1, a: { d: [3, { y: 1, x: 2 }], c: null } }), '{"a":{"c":null,"d":[3,{"x":2,"y":1}]},"b":1}');
  assert.equal(canonicalJson({ a: 1, b: 2 }), canonicalJson({ b: 2, a: 1 }));
});

test("fingerprint is sha256 over the contract fields, order-independent in needs, sensitive to every field", () => {
  const base = { goal: "g", tier: "S1", review_tier: "S4", context: "c", touches: ["a"], acceptance: ["x"], out_of_scope: [], needs: ["b", "a"] };
  const fp = fingerprint(base);
  assert.match(fp, /^[0-9a-f]{64}$/);
  assert.equal(fingerprint({ ...base, needs: ["a", "b"] }), fp);
  for (const [k, v] of Object.entries({ goal: "h", tier: "S2", review_tier: "S5", context: "d", touches: ["z"], acceptance: ["y"], out_of_scope: ["o"], needs: ["a"] })) {
    assert.notEqual(fingerprint({ ...base, [k]: v }), fp, k);
  }
});

test("lockPlan fingerprints every Job; a hand-run bd update by another actor is detected", async () => {
  await newPlan(wf, repo);
  await addDraft(wf, "a");
  await addDraft(wf, "b", ["a"]);
  await assert.rejects(verifyJob(wf, "a"), refusal("job_not_locked", 3), "drafts have no fingerprint");
  await approvePlan(wf);
  await lockPlan(wf);

  assert.deepEqual(Object.keys(readFingerprints(wf)).sort(), ["a", "b"]);
  assert.ok(existsSync(fingerprintsPath(wf)));
  await verifyJob(wf, "a");
  await verifyJob(wf, "b");
  assert.deepEqual(await listTamper(wf), [], "cpd's own writes are never tamper");

  const id = await jobId(wf, "a");
  const r = await rawBd(wf, ["update", id, "-d", "tampered"], "mallory");
  assert.equal(r.code, 0, r.stderr);

  await assert.rejects(verifyJob(wf, "a"), (e: unknown) => {
    assert.ok(e instanceof CpdError);
    assert.equal(e.code, "locked_job_tampered");
    assert.equal(e.exitCode, 3);
    assert.notEqual(e.details.expected, e.details.actual);
    return true;
  });
  await verifyJob(wf, "b"); // untouched Job still verifies

  const tamper = await listTamper(wf);
  assert.equal(tamper.length, 1);
  assert.equal(tamper[0]!.actor, "mallory");
  assert.equal(tamper[0]!.issue, id);
  assert.equal(tamper[0]!.eventType, "updated");
});

test("tampering with Dependencies or metadata is detected too", async () => {
  const bid = await jobId(wf, "b");
  const aid = await jobId(wf, "a");
  await rawBd(wf, ["dep", "remove", bid, aid], "mallory");
  await assert.rejects(verifyJob(wf, "b"), refusal("locked_job_tampered", 3), "needs changed");
  await rawBd(wf, ["dep", "add", bid, aid], "mallory");
  await verifyJob(wf, "b"); // restored by hand: content is identical again
  await rawBd(wf, ["update", bid, "--metadata", '{"tier":"O5"}'], "mallory");
  await assert.rejects(verifyJob(wf, "b"), refusal("locked_job_tampered", 3), "tier changed through metadata");
  assert.ok((await listTamper(wf)).length >= 4);
});

test("cpd's own later writes (cancel, append) do not break verification or count as tamper", async () => {
  const added = await addJob(wf, { key: "app", title: "App", tier: "S2", reviewTier: "S4", spec: spec(), needs: [] }, CATALOG);
  assert.equal(readFingerprints(wf).app, added.fingerprint);
  await verifyJob(wf, "app");
  await cancelJob(wf, "app", "x");
  await verifyJob(wf, "app");
  const before = (await listTamper(wf)).length;
  await bdJson(wf, ["list", "--all", "-n", "0"]);
  assert.equal((await listTamper(wf)).length, before);
  assert.match(readFileSync(fingerprintsPath(wf), "utf8"), /"app"/);
});

test("verifyJob of an unknown key is job_not_found (exit 4)", async () => {
  await assert.rejects(verifyJob(wf, "ghost"), refusal("job_not_found", 4));
});
