import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, rmSync } from "node:fs";
import { scheduleReady, type SchedulePorts } from "../../src/dispatch/schedule.ts";
import { dispatchOne, type DispatchDeps, type Dispatched } from "../../src/dispatch/dispatch-one.ts";
import type { Attempt } from "../../src/ledger/attempts.ts";
import type { Job } from "../../src/ledger/jobs.ts";
import { addDraft, approvePlan, mergeJob, newPlan, sandbox } from "../ledger/helpers.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { readyJobs } from "../../src/ledger/jobs.ts";
import { CpdError, failure, refused } from "../../src/lib/errors.ts";
import { slotsDir } from "../../src/lib/paths.ts";
import { acquireSlot, releaseSlot, setSlotPid } from "../../src/lib/slots.ts";
import { CATALOG } from "./fakes.ts";

// Real slots under a sandboxed CPD_HOME and the real `dispatchOne`; the Ledger and the spawn are fakes.
const { repo } = sandbox();

// Every test starts with the machine's slots free.
beforeEach(() => rmSync(slotsDir(), { recursive: true, force: true }));

const job = (key: string, over: Partial<Job> = {}): Job => ({
  id: `bd-${key}`, key, title: key, status: "queued", tier: "S3", reviewTier: "S4", goal: "g", context: "c",
  touches: ["a"], acceptance: ["a"], outOfScope: [], needs: [], closeReason: null, metadata: { key }, ...over,
});

type Fake = { jobs: Map<string, Job>; spawned: string[]; slots: Map<string, number>; acquires: string[] };

/** A Workflow's Jobs in memory, and the `DispatchDeps` over them: slots are the real machine-wide ones. */
function fake(wf: string, keys: string[], concurrency: number): { f: Fake; deps: DispatchDeps } {
  const f: Fake = { jobs: new Map(keys.map((k) => [k, job(k)])), spawned: [], slots: new Map(), acquires: [] };
  const deps: DispatchDeps = {
    concurrency,
    catalog: CATALOG,
    assertReady() {},
    readJob: async (key) => f.jobs.get(key)!,
    attemptCount: async () => 0,
    acquireSlot: async (who) => {
      f.acquires.push(who.attempt);
      return acquireSlot(concurrency, { wf, attempt: who.attempt });
    },
    releaseSlot,
    setSlotPid,
    startAttempt: async (key) => {
      f.jobs.get(key)!.status = "running";
      return { id: `${key}-a1` } as Attempt;
    },
    failAttempt: async () => {},
    spawnRun(attempt, slot) {
      f.spawned.push(attempt);
      f.slots.set(attempt, slot);
      return { pid: process.pid }; // alive, so the slot stays held until released
    },
  };
  return { f, deps };
}

const ports = (wf: string, f: Fake, deps: DispatchDeps, extra: Partial<SchedulePorts> = {}): SchedulePorts & { reports: string[]; logs: string[] } => {
  const reports: string[] = [];
  const logs: string[] = [];
  return {
    ready: async () => [...f.jobs.values()].filter((j) => j.status === "queued"),
    dispatch: (key) => dispatchOne(wf, key, deps),
    log: (_l, event) => void logs.push(event),
    report: (step, e) => void reports.push(`${step}: ${(e as Error).message}`),
    reports,
    logs,
    ...extra,
  };
};

const heldSlots = () => (existsSync(slotsDir()) ? readdirSync(slotsDir()).filter((n) => /^slot-\d+\.json$/.test(n)).length : 0);

test("cap 2 with 5 ready Jobs starts exactly 2 and stops at the first no_slot; when one finishes, the next starts on the following tick", async () => {
  const { f, deps } = fake("cap", ["j1", "j2", "j3", "j4", "j5"], 2);
  const p = ports("cap", f, deps);

  const t1 = await scheduleReady(p);
  assert.deepEqual(t1.started.map((s) => s.attempt), ["j1-a1", "j2-a1"]);
  assert.equal(t1.noSlot, true);
  assert.deepEqual(f.acquires, ["j1-a1", "j2-a1", "j3-a1"], "j4 and j5 are not even tried once a slot was refused");
  assert.deepEqual(p.reports, [], "no_slot is normal, not an error");
  assert.equal(heldSlots(), 2);

  const t2 = await scheduleReady(p);
  assert.deepEqual(t2.started, [], "nothing finished yet");

  // j1 finishes: its attempt run releases the slot.
  await releaseSlot(f.slots.get("j1-a1")!, "j1-a1");
  f.jobs.get("j1")!.status = "closed";
  const t3 = await scheduleReady(p);
  assert.deepEqual(t3.started.map((s) => s.attempt), ["j3-a1"], "exactly the next one, in creation order");
  assert.equal(heldSlots(), 2);
  assert.deepEqual([...f.jobs.values()].map((j) => j.status), ["closed", "running", "running", "queued", "queued"]);
});

test("Jobs are started in the order `ready` returns them (creation order)", async () => {
  const { f, deps } = fake("order", ["zeta", "alpha", "mid"], 5);
  const t = await scheduleReady(ports("order", f, deps));
  assert.deepEqual(t.started.map((s) => s.attempt), ["zeta-a1", "alpha-a1", "mid-a1"]);
});

test("a Job whose Dependency is not Merged never starts (real Ledger: bd ready)", async () => {
  await newPlan("deps", repo);
  await addDraft("deps", "a");
  await addDraft("deps", "b", ["a"]);
  await addDraft("deps", "c", ["a", "b"]);
  await approvePlan("deps");
  await lockPlan("deps");

  const dispatched: string[] = [];
  const p: SchedulePorts = {
    ready: () => readyJobs("deps"),
    dispatch: async (key) => {
      dispatched.push(key);
      return { attempt: `${key}-a1`, slot: 0, pid: 1, tier: "S3" } satisfies Dispatched;
    },
    log: () => {},
    report: () => {},
  };
  await scheduleReady(p);
  assert.deepEqual(dispatched, ["a"], "b and c wait for a");
  await scheduleReady(p);
  assert.deepEqual(dispatched, ["a", "a"], "a is still queued here (the fake dispatch starts nothing), b and c still never start");

  await mergeJob("deps", "a");
  dispatched.length = 0;
  await scheduleReady(p);
  assert.deepEqual(dispatched, ["b"], "c still needs b");
  await mergeJob("deps", "b");
  dispatched.length = 0;
  await scheduleReady(p);
  assert.deepEqual(dispatched, ["c"]);
});

test("a lost race (cas_lost, job_not_ready) is logged and the next Job is tried; it is not reported", async () => {
  const { f, deps } = fake("race", ["a", "b", "c"], 5);
  const start = deps.startAttempt;
  deps.startAttempt = async (key, tier) => {
    if (key === "a") return null; // somebody ran `cpd job dispatch a` first
    if (key === "b") throw refused("job_not_ready", "blocked");
    return start(key, tier);
  };
  const p = ports("race", f, deps);
  const t = await scheduleReady(p);
  assert.deepEqual(t.started.map((s) => s.attempt), ["c-a1"]);
  assert.deepEqual(p.reports, []);
  assert.equal(p.logs.filter((l) => l === "dispatch_skipped").length, 2);
  assert.equal(heldSlots(), 1, "the slots of the lost races were given back");
});

test("any other refusal is reported and the next Job is still tried", async () => {
  const { f, deps } = fake("refused", ["a", "b"], 5);
  const read = deps.readJob;
  deps.readJob = async (key) => {
    if (key === "a") throw refused("locked_job_tampered", "Job a was edited behind cpd's back");
    return read(key);
  };
  const p = ports("refused", f, deps);
  const t = await scheduleReady(p);
  assert.deepEqual(t.started.map((s) => s.attempt), ["b-a1"]);
  assert.equal(p.reports.length, 1);
  assert.match(p.reports[0]!, /schedule a: Job a was edited/);
});

test("a tool failure stops this tick's scheduling and propagates to the tick", async () => {
  const { f, deps } = fake("tool", ["a", "b"], 5);
  deps.acquireSlot = async () => {
    throw failure("slot_corrupt", "slot file is not valid JSON");
  };
  await assert.rejects(scheduleReady(ports("tool", f, deps)), (e: unknown) => e instanceof CpdError && e.code === "slot_corrupt");
  assert.deepEqual(f.spawned, []);
});

test("machine-wide: two Workflows sharing CPD_HOME never hold more slots than `concurrency`", async () => {
  const cap = 3;
  const a = fake("wfa", ["a1", "a2", "a3", "a4"], cap);
  const b = fake("wfb", ["b1", "b2", "b3", "b4"], cap);
  const [ra, rb] = await Promise.all([scheduleReady(ports("wfa", a.f, a.deps)), scheduleReady(ports("wfb", b.f, b.deps))]);
  assert.equal(ra.started.length + rb.started.length, cap, "two ticks at the same moment start `concurrency` Attempts between them");
  assert.ok(heldSlots() <= cap);
  const again = await Promise.all([scheduleReady(ports("wfa", a.f, a.deps)), scheduleReady(ports("wfb", b.f, b.deps))]);
  assert.equal(again[0].started.length + again[1].started.length, 0);
  assert.equal(heldSlots(), cap);
});
