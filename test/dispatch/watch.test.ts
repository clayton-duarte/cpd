import { beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  ERROR_REPEAT_MS,
  allMerged,
  claimWatch,
  heartbeatWatch,
  isFatal,
  listEscalationFiles,
  makeReporter,
  newState,
  prPollDue,
  readWatchFile,
  realIdentity,
  realWatchDeps,
  releaseWatch,
  runLoop,
  runAlive,
  tick,
  type WatchDeps,
  type WatchIdentity,
} from "../../src/dispatch/watch.ts";
import { dispatchOne, type DispatchDeps } from "../../src/dispatch/dispatch-one.ts";
import type { LedgerEscalation } from "../../src/dispatch/liveness.ts";
import { getAttempt, listAttempts, startAttempt, type Attempt } from "../../src/ledger/attempts.ts";
import { recordEscalation } from "../../src/ledger/escalations.ts";
import { cancelJob, getJobByKey, type Job } from "../../src/ledger/jobs.ts";
import { lockPlan } from "../../src/ledger/plan.ts";
import { readWorkflow, writeWorkflow, type Phase } from "../../src/ledger/workflow.ts";
import { CpdError, failure, refused } from "../../src/lib/errors.ts";
import { cpdHome, escalationsDir, slotPath, slotsDir, watchPath } from "../../src/lib/paths.ts";
import { locatePi } from "../../src/lib/pi-install.ts";
import { acquireSlot, releaseSlot, setSlotPid } from "../../src/lib/slots.ts";
import { loadConfig } from "../../src/lib/config.ts";
import type { NotifyInput } from "../../src/queue/lead-queue.ts";
import { addDraft, approvePlan, newPlan, sandbox } from "../ledger/helpers.ts";
import { CATALOG } from "./fakes.ts";

const { repo } = sandbox();
beforeEach(() => rmSync(slotsDir(), { recursive: true, force: true }));

const MIN = 60_000;
const T0 = Date.parse("2026-01-01T00:00:00.000Z");
const at = (minutes: number) => T0 + minutes * MIN;
const iso = (ms: number) => new Date(ms).toISOString();

// ---------- watch.json: one live instance ----------

const identity = (pid: number, now: number, alive: (pid: number) => boolean = () => true, staleAfterMs = 5 * MIN): WatchIdentity => ({ pid, now: () => now, alive, staleAfterMs });
const notFoundYet = (wf: string) => assert.equal(existsSync(watchPath(wf)), false);

test("claim: a free Workflow gets watch.json {pid, startedAt, heartbeatAt}", async () => {
  notFoundYet("claim-a");
  const r = await claimWatch("claim-a", identity(4242, at(0)));
  assert.equal(r.tookOver, null);
  assert.deepEqual(JSON.parse(readFileSync(watchPath("claim-a"), "utf8")), { pid: 4242, startedAt: iso(at(0)), heartbeatAt: iso(at(0)) });
});

test("a second watch while the first is alive is refused: watch_running (exit 3), watch.json untouched", async () => {
  await claimWatch("claim-b", identity(100, at(0)));
  const before = readFileSync(watchPath("claim-b"), "utf8");
  await assert.rejects(claimWatch("claim-b", identity(200, at(1))), (e: unknown) => e instanceof CpdError && e.code === "watch_running" && e.exitCode === 3 && e.details.pid === 100);
  assert.equal(readFileSync(watchPath("claim-b"), "utf8"), before);
});

test("a stale watch.json (its pid is dead) is taken over, and the previous holder is returned", async () => {
  await claimWatch("claim-c", identity(100, at(0)));
  const r = await claimWatch("claim-c", identity(200, at(1), (pid) => pid !== 100));
  assert.equal(r.tookOver?.pid, 100);
  assert.equal(readWatchFile("claim-c")?.pid, 200);
});

test("a live pid whose heartbeat is old (a reused pid) is stale too; a fresh heartbeat is not", async () => {
  await claimWatch("claim-d", identity(100, at(0)));
  await assert.rejects(claimWatch("claim-d", identity(200, at(4))), (e: unknown) => e instanceof CpdError && e.code === "watch_running");
  const r = await claimWatch("claim-d", identity(200, at(6)));
  assert.equal(r.tookOver?.pid, 100);
});

test("heartbeat refreshes heartbeatAt; once another watch took over, the old one gets watch_superseded", async () => {
  const mine = identity(100, at(0));
  await claimWatch("claim-e", mine);
  await heartbeatWatch("claim-e", { ...mine, now: () => at(3) });
  assert.deepEqual([readWatchFile("claim-e")?.startedAt, readWatchFile("claim-e")?.heartbeatAt], [iso(at(0)), iso(at(3))]);
  await claimWatch("claim-e", identity(200, at(4), (pid) => pid !== 100));
  await assert.rejects(heartbeatWatch("claim-e", mine), (e: unknown) => e instanceof CpdError && e.code === "watch_superseded");
  assert.equal(isFatal(new CpdError("watch_superseded", "x", 1)), true);
});

test("release removes watch.json only when it is ours", async () => {
  await claimWatch("claim-f", identity(100, at(0)));
  await releaseWatch("claim-f", identity(200, at(0)));
  assert.equal(readWatchFile("claim-f")?.pid, 100, "someone else's is left alone");
  await releaseWatch("claim-f", identity(100, at(0)));
  notFoundYet("claim-f");
  await releaseWatch("claim-f", identity(100, at(0))); // idempotent
});

test("a watch.json that does not parse is watch_state_invalid, never silently taken over", async () => {
  mkdirSync(join(cpdHome(), "workflows", "claim-g"), { recursive: true });
  writeFileSync(watchPath("claim-g"), "{not json");
  await assert.rejects(claimWatch("claim-g", identity(1, at(0))), (e: unknown) => e instanceof CpdError && e.code === "watch_state_invalid");
  writeFileSync(watchPath("claim-g"), JSON.stringify({ pid: "x" }));
  await assert.rejects(claimWatch("claim-g", identity(1, at(0))), (e: unknown) => e instanceof CpdError && e.code === "watch_state_invalid");
});

test("runAlive: a slot holds the pid of an Attempt's run; a dead pid, a free slot and no slot directory all mean not alive", async () => {
  assert.equal(runAlive("x", "api-a1"), false, "no slot directory");
  const slot = await acquireSlot(2, { wf: "x", attempt: "api-a1" });
  await setSlotPid(slot!, "api-a1", process.pid);
  assert.equal(runAlive("x", "api-a1"), true);
  assert.equal(runAlive("x", "api-a2"), false, "another Attempt");
  await setSlotPid(slot!, "api-a1", 2 ** 22 + 12345);
  assert.equal(runAlive("x", "api-a1"), false, "its process is gone");
  await releaseSlot(slot!, "api-a1");
  assert.equal(runAlive("x", "api-a1"), false);
  writeFileSync(slotPath(0), "{broken");
  assert.throws(() => runAlive("x", "api-a1"), (e: unknown) => e instanceof CpdError && e.code === "slot_corrupt");
});

/** Slots 0, 1, … held by `api-a1` of the given Workflows (in this order), whose `attempt run` is alive or dead. */
async function holdSlots(...holders: [wf: string, alive: boolean][]): Promise<void> {
  rmSync(slotsDir(), { recursive: true, force: true });
  const taken: (number | null)[] = [];
  for (const [wf] of holders) taken.push(await acquireSlot(holders.length, { wf, attempt: "api-a1" })); // every holder is this live process for now, so no slot is reclaimed as stale
  for (const [n, [, alive]] of holders.entries()) await setSlotPid(taken[n]!, "api-a1", alive ? process.pid : 2 ** 22 + 12345);
}

test("runAlive: Attempt ids repeat across Workflows sharing CPD_HOME, so only a slot of this Workflow counts, in either slot order", async () => {
  for (const flip of [false, true]) {
    const order = (...slots: [string, boolean][]) => (flip ? slots.reverse() : slots);
    await holdSlots(...order(["other", true]));
    assert.equal(runAlive("x", "api-a1"), false, `only the api-a1 of another Workflow is running (flip ${flip})`);
    await holdSlots(...order(["other", true], ["x", true]));
    assert.equal(runAlive("x", "api-a1"), true, `ours is running (flip ${flip})`);
    await holdSlots(...order(["other", true], ["x", false]));
    assert.equal(runAlive("x", "api-a1"), false, `ours died: the live api-a1 of the other Workflow must not hide the crash (flip ${flip})`);
    await holdSlots(...order(["other", false], ["x", true]));
    assert.equal(runAlive("x", "api-a1"), true, `ours is alive: the dead api-a1 of the other Workflow must not make it look crashed (flip ${flip})`);
  }
});

// ---------- the tick, against a fake world ----------

const job = (key: string, over: Partial<Job> = {}): Job => ({
  id: `bd-${key}`, key, title: key, status: "queued", tier: "S3", reviewTier: "S4", goal: "g", context: "c",
  touches: ["a"], acceptance: ["a"], outOfScope: [], needs: [], closeReason: null, metadata: { key, tier: "S3" }, ...over,
});
/** An `in_progress` Attempt with a Worker (agent `c<key>`, pane `w1:p<n>`), or, with `noWorker`, one whose Worker was not started yet. */
const attempt = (key: string, n: number, over: Partial<Attempt> & { noWorker?: boolean } = {}): Attempt => {
  const { noWorker, ...rest } = over;
  const worker = noWorker ? {} : { agent: `c${key}`, worker_pane: `w1:p${n}` };
  return {
    id: `${key}-a${n}`, issue: `bd-${key}-a${n}`, job: key, jobIssue: `bd-${key}`, number: n, tier: "S3", status: "in_progress", outcome: null, cause: null,
    worktree: `/wt/${key}-a${n}`, branch: `cpd/demo--${key}-a${n}`, agent: noWorker ? null : `c${key}`, sessionPath: null, startedAt: iso(T0), submittedAt: null, endedAt: null, closeReason: null,
    metadata: { number: n, tier: "S3", started_at: iso(T0), ...worker },
    ...rest,
  };
};

type PollResult = { ok: true } | { ok: false; code: string; message: string };
type W = {
  now: number;
  phase: Phase;
  jobs: Job[];
  attempts: Attempt[];
  escalations: LedgerEscalation[];
  running: Set<string>;
  agents: Set<string>;
  freeSlots: number;
  events: string[];
  messages: NotifyInput[];
  logs: string[];
  claimed: boolean;
  polls: number[];
  /** What `cpd pr poll` returns, in order (the last one repeats); `done` moves the phase to done. */
  pr: (PollResult & { done?: boolean })[];
  failPhase?: Error;
  failAttempts?: Error;
};

function world(over: Partial<W> = {}): W {
  return {
    now: at(1), phase: "dispatch", jobs: [], attempts: [], escalations: [], running: new Set(), agents: new Set(), freeSlots: 2,
    events: [], messages: [], logs: [], claimed: false, polls: [], pr: [{ ok: true }], ...over,
  };
}

function deps(w: W, o: { prPollSeconds?: number; retryCap?: number; dispatch?: WatchDeps["dispatch"] } = {}): WatchDeps {
  const jobOf = (key: string) => w.jobs.find((j) => j.key === key)!;
  const attemptOf = (id: string) => w.attempts.find((a) => a.id === id)!;
  const clock = { now: () => w.now, sleep: async (ms: number) => void (w.now += ms) };
  const log = (_l: string, event: string) => void w.logs.push(event);
  const queue = { notify: (m: NotifyInput) => void w.messages.push(m) };
  const parts: Omit<WatchDeps, "report"> = {
    wf: "demo",
    config: { attemptTimeoutMinutes: 30, retryCap: o.retryCap ?? 2, tickSeconds: 5, prPollSeconds: o.prPollSeconds ?? 120 },
    catalog: CATALOG,
    clock,
    log,
    queue,
    ledger: {
      async phase() {
        if (w.failPhase) throw w.failPhase;
        return w.phase;
      },
      jobs: async () => structuredClone(w.jobs),
      async attempts() {
        if (w.failAttempts) throw w.failAttempts;
        return structuredClone(w.attempts);
      },
      attempt: async (id) => structuredClone(attemptOf(id)),
      escalations: async () => structuredClone(w.escalations),
      async markTimedOut(id) {
        w.events.push(`ledger.timed_out ${id}`);
        attemptOf(id).metadata.timed_out = true;
      },
      async failAttempt(id, cause, f) {
        w.events.push(`ledger.fail ${id} ${cause} requeue=${f.requeue}`);
        const a = attemptOf(id);
        a.status = "closed";
        w.running.delete(id);
        jobOf(a.job).status = cause === "cancelled" ? "cancelled" : f.requeue ? "queued" : "failed";
      },
      async cancelJob(key) {
        w.events.push(`ledger.cancelJob ${key}`);
        jobOf(key).status = "cancelled";
      },
      expireEscalation: async () => {},
      recordEscalation: async () => {},
      recordAnswer: async () => {},
      async claimAllMerged() {
        w.events.push("ledger.claimAllMerged");
        const first = !w.claimed;
        w.claimed = true;
        return first;
      },
    },
    herdr: {
      agentAlive: async (name) => w.agents.has(name),
      async closePane(id) {
        w.events.push(`herdr.closePane ${id}`);
      },
    },
    runAlive: (id) => w.running.has(id),
    expireQuestion: () => {},
    files: { list: () => ({ questions: [], answers: [], expired: [] }), question: () => null, answer: () => null, expireOrphans: () => [] },
    async ready() {
      return w.jobs.filter((j) => j.status === "queued" && j.needs.every((n) => jobOf(n).status === "closed"));
    },
    dispatch:
      o.dispatch ??
      (async (key) => {
        if (w.freeSlots === 0) throw refused("no_slot", "no free concurrency slot");
        w.freeSlots--;
        const n = w.attempts.filter((a) => a.job === key).length + 1;
        jobOf(key).status = "running";
        w.attempts.push(attempt(key, n, { noWorker: true, startedAt: iso(w.now) }));
        w.running.add(`${key}-a${n}`);
        w.events.push(`dispatch ${key}-a${n}`);
        return { attempt: `${key}-a${n}`, slot: 0, pid: 1, tier: "S3" };
      }),
    pr: {
      async poll() {
        w.polls.push(w.now);
        const r = w.pr.length > 1 ? w.pr.shift()! : w.pr[0]!;
        if (r.done) w.phase = "done";
        return r.ok ? { ok: true } : r;
      },
    },
    heartbeat: async () => {
      w.events.push("heartbeat");
    },
  };
  return { ...parts, report: makeReporter({ wf: "demo", clock, log, queue }) };
}

const kinds = (w: W) => w.messages.map((m) => m.kind);

test("cap 2 with 5 ready Jobs starts exactly 2; when one finishes, the next starts on the following tick", async () => {
  const w = world({ jobs: ["j1", "j2", "j3", "j4", "j5"].map((k) => job(k)) });
  const d = deps(w);
  const s = newState();
  assert.deepEqual((await tick(d, s)).started, ["j1-a1", "j2-a1"]);
  assert.deepEqual((await tick(d, s)).started, []);
  w.freeSlots++; // j1 merged: its attempt run released the slot
  w.jobs[0]!.status = "closed";
  w.attempts[0]!.status = "closed";
  w.running.delete("j1-a1");
  assert.deepEqual((await tick(d, s)).started, ["j3-a1"]);
  assert.deepEqual(w.jobs.map((j) => j.status), ["closed", "running", "running", "queued", "queued"]);
});

test("a Job whose blocker is not Merged never starts, however many ticks pass", async () => {
  const w = world({ jobs: [job("a", { status: "running" }), job("b", { needs: ["a"] })], attempts: [attempt("a", 1)], running: new Set(["a-a1"]), agents: new Set(["ca"]) });
  const d = deps(w);
  const s = newState();
  for (let i = 0; i < 3; i++) assert.deepEqual((await tick(d, s)).started, []);
  w.jobs[0]!.status = "closed";
  w.attempts[0]!.status = "closed";
  assert.deepEqual((await tick(d, s)).started, ["b-a1"]);
});

test("a timeout in a tick marks timed_out and closes the Worker pane", async () => {
  const w = world({ now: at(31), jobs: [job("api", { status: "running" })], attempts: [attempt("api", 1)], running: new Set(["api-a1"]) });
  const r = await tick(deps(w), newState());
  assert.deepEqual(r.timedOut, ["api-a1"]);
  assert.deepEqual(w.events, ["ledger.timed_out api-a1", "herdr.closePane w1:p1"]);
  assert.equal(w.attempts[0]!.metadata.timed_out, true);
});

test("an Escalation open for 10 minutes extends the timeout by 10 minutes", async () => {
  const asked: LedgerEscalation[] = [{ id: "api-a1-q1", attempt: "api-a1", open: false, from: at(5), to: at(15) }];
  const w = world({ now: at(35), jobs: [job("api", { status: "running" })], attempts: [attempt("api", 1)], running: new Set(["api-a1"]), escalations: asked });
  const d = deps(w);
  assert.deepEqual((await tick(d, newState())).timedOut, [], "35 minutes elapsed, 25 active");
  w.now = at(41);
  assert.deepEqual((await tick(d, newState())).timedOut, ["api-a1"]);
});

test("a crash in a tick fails the Attempt as crashed and requeues it via the ladder; the next Attempt starts in the same tick", async () => {
  const w = world({ jobs: [job("api", { status: "running" })], attempts: [attempt("api", 1)] }); // its agent is not in herdr, no attempt run is alive
  const r = await tick(deps(w), newState());
  assert.deepEqual(r.crashed, ["api-a1"]);
  assert.deepEqual(r.started, ["api-a2"]);
  assert.deepEqual(w.events, ["herdr.closePane w1:p1", "ledger.fail api-a1 crashed requeue=true", "dispatch api-a2"]);
  assert.deepEqual(kinds(w), ["attempt-retry"]);
});

test("cancel_requested in a tick stops the Attempt and cancels the Job (and the Job is not started again)", async () => {
  const w = world({
    jobs: [job("api", { status: "running", metadata: { key: "api", cancel_requested: iso(at(0)), cancel_reason: "descoped" } })],
    attempts: [attempt("api", 1)],
    running: new Set(["api-a1"]),
    agents: new Set(["capi"]),
  });
  const r = await tick(deps(w), newState());
  assert.deepEqual([r.cancelled, r.started], [["api-a1"], []]);
  assert.deepEqual(w.events, ["herdr.closePane w1:p1", "ledger.fail api-a1 cancelled requeue=false"]);
  assert.equal(w.jobs[0]!.status, "cancelled");
  assert.deepEqual(kinds(w), []);
});

test("all-merged is sent exactly once: not again on later ticks, and not again by a restarted watch", async () => {
  const w = world({ jobs: [job("a", { status: "closed" }), job("b", { status: "closed" }), job("c", { status: "cancelled" })] });
  const d = deps(w);
  const s = newState();
  const first = await tick(d, s);
  assert.equal(first.allMerged, true);
  assert.deepEqual(kinds(w), ["all-merged"]);
  assert.match(w.messages[0]!.text, /cpd pr open --wf demo/);
  assert.deepEqual(w.messages[0]!.data, { merged: ["a", "b"], cancelled: ["c"] });
  await tick(d, s);
  await tick(d, s);
  assert.equal(w.events.filter((e) => e === "ledger.claimAllMerged").length, 1, "later ticks do not even ask");
  assert.deepEqual(kinds(w), ["all-merged"]);
  await tick(d, newState()); // a restarted watch: the write-once metadata says it was sent
  assert.equal(w.events.filter((e) => e === "ledger.claimAllMerged").length, 2);
  assert.deepEqual(kinds(w), ["all-merged"]);
});

test("all-merged waits while any Job is queued, running, in review, failed or draft, and needs at least one Merged Job", () => {
  const status = (...s: Job["status"][]) => s.map((x) => ({ status: x }));
  assert.equal(allMerged(status("closed", "closed")), true);
  assert.equal(allMerged(status("closed", "cancelled")), true);
  assert.equal(allMerged(status("cancelled", "cancelled")), false, "nothing was Merged");
  assert.equal(allMerged([]), false);
  for (const open of ["queued", "running", "in_review", "failed", "draft"] as const) assert.equal(allMerged(status("closed", open)), false, open);
});

test("all-merged is only for phase dispatch, and not on a tick that changed things", async () => {
  const done = world({ phase: "pr", jobs: [job("a", { status: "closed" })] });
  await tick(deps(done), newState());
  assert.deepEqual(kinds(done), []);

  const busy = world({ jobs: [job("a", { status: "closed" }), job("b", { status: "running", metadata: { key: "b", cancel_requested: "x", cancel_reason: "r" } })], attempts: [attempt("b", 1)], running: new Set(["b-a1"]), agents: new Set(["cb"]) });
  const r = await tick(deps(busy), newState());
  assert.deepEqual([r.cancelled, r.allMerged], [["b-a1"], false], "the Jobs read before the cancel are stale: the next tick decides");
  assert.deepEqual((await tick(deps(busy), newState())).allMerged, true);
});

test("PR polling follows prPollSeconds: at once in phase pr, then every interval; never in other phases", async () => {
  assert.equal(prPollDue("pr", 0, null, 120_000), true);
  assert.equal(prPollDue("pr", 119_999, 0, 120_000), false);
  assert.equal(prPollDue("pr", 120_000, 0, 120_000), true);
  assert.equal(prPollDue("dispatch", 0, null, 120_000), false);
  assert.equal(prPollDue("done", 0, null, 120_000), false);

  const w = world({ phase: "pr", now: at(0), jobs: [job("a", { status: "closed" })], pr: [{ ok: true }] });
  const d = deps(w, { prPollSeconds: 120 });
  const s = newState();
  const seen: number[] = [];
  for (let i = 0; i < 30; i++) {
    const r = await tick(d, s); // each tick is 5 seconds after the last
    if (r.polledPr) seen.push((w.now - at(0)) / 1000);
    w.now += 5_000;
  }
  assert.deepEqual(seen, [0, 120], "polled at 0 s and 120 s, and not between (30 ticks span 145 s)");
  assert.deepEqual(w.polls, [at(0), at(2)]);
});

test("a tick that sees the PR merge (phase done) ends the loop", async () => {
  const w = world({ phase: "pr", jobs: [job("a", { status: "closed" })], pr: [{ ok: true, done: true }] });
  const r = await tick(deps(w), newState());
  assert.deepEqual([r.polledPr, r.done], [true, true]);
  const again = await tick(deps(w), newState());
  assert.deepEqual([again.done, again.polledPr], [true, false], "a done Workflow is not polled");
});

test("a failing or not_implemented PR poll is logged and reported (rate limited), never fatal", async () => {
  const w = world({ phase: "pr", now: at(0), jobs: [job("a", { status: "closed" })], pr: [{ ok: false, code: "not_implemented", message: "cpd pr poll is not implemented" }] });
  const d = deps(w, { prPollSeconds: 60 });
  const s = newState();
  for (let i = 0; i < 5; i++) {
    const r = await tick(d, s);
    assert.equal(r.done, false);
    w.now += 61_000; // five polls in five minutes
  }
  assert.equal(w.polls.length, 5);
  assert.equal(w.messages.length, 1, "one error message for five identical failures");
  assert.equal(w.messages[0]!.kind, "error");
  assert.match(w.messages[0]!.text, /not_implemented/);
  assert.equal(w.logs.filter((l) => l === "watch_error").length, 5);
});

// ---------- errors ----------

test("a transient tool error in one step is reported and the other steps still run", async () => {
  const w = world({ jobs: [job("j1")], failAttempts: failure("bd_failed", "bd list exited 1: database is locked") });
  const r = await tick(deps(w), newState());
  assert.deepEqual(r.started, ["j1-a1"], "scheduling went on although liveness could not read the Attempts");
  assert.deepEqual(kinds(w), ["error"]);
  assert.match(w.messages[0]!.text, /step liveness.*database is locked.*bd_failed/);
});

test("the same error is sent once per 10 minutes; a different error is sent at once", async () => {
  const w = world();
  const report = makeReporter({ wf: "demo", clock: { now: () => w.now, sleep: async () => {} }, log: () => {}, queue: { notify: (m) => void w.messages.push(m) } });
  const e1 = failure("bd_failed", "bd is down");
  for (let i = 0; i < 20; i++) {
    report("liveness", e1);
    w.now += 20_000; // 20 failures in 400 s
  }
  assert.equal(w.messages.length, 1);
  report("schedule", e1);
  report("liveness", failure("herdr_server_down", "no herdr"));
  assert.equal(w.messages.length, 3, "another step, another code: distinct");
  w.now = at(1) + ERROR_REPEAT_MS - 1;
  report("liveness", e1);
  assert.equal(w.messages.length, 3, "still inside the 10 minutes of the first one");
  w.now = at(1) + ERROR_REPEAT_MS + 20_000 * 20;
  report("liveness", e1);
  assert.equal(w.messages.length, 4, "after 10 minutes it may be sent again");
  for (const m of w.messages) assert.equal(m.kind, "error");
});

test("a message that cannot be delivered does not stop the loop", () => {
  const logs: string[] = [];
  const report = makeReporter({ wf: "demo", clock: { now: () => 0, sleep: async () => {} }, log: (_l, e) => void logs.push(e), queue: { notify: () => { throw refused("lead_not_attached", "no Lead"); } } });
  report("liveness", failure("bd_failed", "x"));
  assert.deepEqual(logs, ["watch_error", "notify_failed"]);
});

test("config or Ledger corruption is fatal; tool failures and refusals are not", () => {
  for (const code of ["config_missing", "config_invalid", "ledger_invalid", "workflow_invalid", "workflow_not_found"]) assert.equal(isFatal(new CpdError(code, "x", 1)), true, code);
  for (const code of ["bd_failed", "herdr_server_down", "lock_timeout", "cas_lost", "pr_poll_failed", "slot_corrupt"]) assert.equal(isFatal(new CpdError(code, "x", 1)), false, code);
  assert.equal(isFatal(new Error("boom")), false);
});

test("the loop keeps ticking through transient errors and exits 0-style (resolves) when the phase is done", async () => {
  const w = world({ failPhase: failure("bd_failed", "bd is down") });
  const d = deps(w);
  let sleeps = 0;
  d.clock.sleep = async (ms) => {
    w.now += ms;
    if (++sleeps === 3) {
      w.failPhase = undefined;
      w.phase = "done";
    }
  };
  const r = await runLoop(d);
  assert.equal(r.ticks, 1, "three ticks failed, the fourth saw done");
  assert.equal(w.events.filter((e) => e === "heartbeat").length, 4);
  assert.equal(w.messages.filter((m) => m.kind === "error").length, 1, "three identical failures, one message");
  assert.equal(sleeps, 3);
  assert.equal(w.now, at(1) + 3 * 5000, "it sleeps tickSeconds between ticks");
});

test("Ledger corruption ends the loop, after exactly one error message to the Lead", async () => {
  const w = world({ failPhase: failure("ledger_invalid", "workflow issue has no valid phase") });
  await assert.rejects(runLoop(deps(w)), (e: unknown) => e instanceof CpdError && e.code === "ledger_invalid");
  assert.deepEqual(kinds(w), ["error"]);
  assert.equal(w.messages[0]!.data?.fatal, true);
  assert.match(w.messages[0]!.text, /stopped.*workflow issue has no valid phase.*cpd watch --wf demo/);
});

test("fatal corruption inside a step also ends the loop with one message", async () => {
  const w = world({ jobs: [job("j1")], failAttempts: failure("ledger_invalid", "Attempt issue bd-1 does not have the expected shape") });
  await assert.rejects(runLoop(deps(w)), (e: unknown) => e instanceof CpdError && e.code === "ledger_invalid");
  assert.deepEqual(kinds(w), ["error"]);
  assert.deepEqual(w.events.filter((e) => e.startsWith("dispatch")), [], "nothing was scheduled on a corrupt Ledger");
});

test("a watch that lost watch.json stops quietly: it is superseded, not broken", async () => {
  const w = world();
  const d = deps(w);
  d.heartbeat = async () => {
    throw failure("watch_superseded", "taken over");
  };
  await assert.rejects(runLoop(d), (e: unknown) => e instanceof CpdError && e.code === "watch_superseded");
  assert.deepEqual(kinds(w), []);
  assert.ok(w.logs.includes("watch_stopped"));
});

// ---------- machine-wide cap across Workflows ----------

function slotDeps(wf: string, jobs: Job[], concurrency: number): DispatchDeps {
  return {
    concurrency,
    catalog: CATALOG,
    assertReady() {},
    readJob: async (key) => jobs.find((j) => j.key === key)!,
    attemptCount: async () => 0,
    acquireSlot: (who) => acquireSlot(concurrency, { wf, attempt: who.attempt }),
    releaseSlot,
    setSlotPid,
    startAttempt: async (key) => {
      jobs.find((j) => j.key === key)!.status = "running";
      return { id: `${key}-a1` } as Attempt;
    },
    failAttempt: async () => {},
    spawnRun: () => ({ pid: process.pid }),
  };
}

test("two watch ticks on two Workflows sharing CPD_HOME never run more Attempts than `concurrency`", async () => {
  const cap = 3;
  const a = world({ jobs: ["a1", "a2", "a3", "a4"].map((k) => job(k)) });
  const b = world({ jobs: ["b1", "b2", "b3", "b4"].map((k) => job(k)) });
  const da = deps(a, { dispatch: (key) => dispatchOne("wfa", key, slotDeps("wfa", a.jobs, cap)) });
  const db = deps(b, { dispatch: (key) => dispatchOne("wfb", key, slotDeps("wfb", b.jobs, cap)) });
  const [ra, rb] = await Promise.all([tick(da, newState()), tick(db, newState())]);
  assert.equal(ra.started.length + rb.started.length, cap);
  const running = [...a.jobs, ...b.jobs].filter((j) => j.status === "running").length;
  assert.equal(running, cap);
  assert.equal(readdirSync(slotsDir()).filter((n) => /^slot-\d+\.json$/.test(n)).length, cap);
  const [ra2, rb2] = [await tick(da, newState()), await tick(db, newState())];
  assert.equal(ra2.started.length + rb2.started.length, 0, "the cap holds on the next tick too");
  assert.deepEqual(kinds(a).concat(kinds(b)), [], "no_slot is not an error");
});

// ---------- files ----------

test("escalation files: an <id>.expired.json is listed as expired, never as an open question; stray names are ignored", () => {
  const dir = escalationsDir("files");
  mkdirSync(dir, { recursive: true });
  for (const n of ["api-a1-q1.q.json", "api-a1-q1.a.json", "api-a1-q2.q.json", "api-a1-q3.expired.json", "api-a2-q1.expired.json", "notes.q.json", "api-a1-q9.q.json.123.tmp", "README"]) writeFileSync(join(dir, n), "{}");
  assert.deepEqual(listEscalationFiles("files"), {
    questions: ["api-a1-q1", "api-a1-q2"],
    answers: ["api-a1-q1"],
    expired: ["api-a1-q3", "api-a2-q1"],
  });
  assert.deepEqual(listEscalationFiles("nothing-here"), { questions: [], answers: [], expired: [] });
});

// ---------- real Ledger ----------

const CONFIG = {
  version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5, prPollSeconds: 120, herdrSession: "fake-never-the-default",
  tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
  leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
};

test("real Ledger, one tick: a cancel with an open Escalation, a crash and a timeout are each handled; all-merged is write-once", { timeout: 600_000 }, async () => {
  process.env.PI_CODING_AGENT_DIR = locatePi().agentDir;
  mkdirSync(cpdHome(), { recursive: true });
  writeFileSync(join(cpdHome(), "config.json"), JSON.stringify(CONFIG));
  const wf = "real";
  await newPlan(wf, repo);
  for (const k of ["cx", "cr", "to"]) await addDraft(wf, k, [], "H1");
  await approvePlan(wf);
  await lockPlan(wf);
  const queueDir = join(cpdHome(), "leads", "lead1", "queue");
  const w0 = readWorkflow(wf);
  await writeWorkflow(wf, { ...w0, leadWorktree: repo, lead: { paneId: "w1:p1", sessionPath: "/s.jsonl", sessionId: "lead1", queueDir, attachedAt: iso(T0) } });
  for (const k of ["cx", "cr", "to"]) assert.equal((await startAttempt(wf, k, { tier: "H1" }))?.id, `${k}-a1`);

  // cx: the Lead cancels while the Worker waits for an answer.
  const asked = await recordEscalation(wf, "cx-a1", { escId: "cx-a1-q1", question: "which?", askedAt: new Date().toISOString() });
  assert.equal(asked.created, true);
  mkdirSync(escalationsDir(wf), { recursive: true });
  writeFileSync(join(escalationsDir(wf), "cx-a1-q1.q.json"), JSON.stringify({ v: 1, id: "cx-a1-q1", wf, attempt: "cx-a1", question: "which?", askedAt: new Date().toISOString(), askerPid: process.pid }));
  await cancelJob(wf, "cx", "descoped");
  // to: its attempt run is alive (a slot with a live pid), and the clock says 31 minutes passed.
  const slot = await acquireSlot(3, { wf, attempt: "to-a1" });
  await setSlotPid(slot!, "to-a1", process.pid);

  const real = await realWatchDeps(wf, loadConfig(), realIdentity(loadConfig()));
  const d: WatchDeps = { ...real, clock: { now: () => Date.now() + 31 * MIN, sleep: async () => {} }, ready: async () => [] };
  const r = await tick(d, newState());
  assert.deepEqual([r.cancelled, r.crashed, r.timedOut], [["cx-a1"], ["cr-a1"], ["to-a1"]]);

  const cx = await getAttempt(wf, "cx-a1");
  assert.deepEqual([cx.status, cx.outcome, cx.cause], ["closed", "cancelled", "cancelled"]);
  assert.equal((await getJobByKey(wf, "cx")).status, "cancelled");
  assert.ok(existsSync(join(escalationsDir(wf), "cx-a1-q1.expired.json")), "the question was renamed, not deleted");
  assert.equal(existsSync(join(escalationsDir(wf), "cx-a1-q1.q.json")), false);

  const cr = await getAttempt(wf, "cr-a1");
  assert.deepEqual([cr.status, cr.outcome, cr.cause], ["closed", "failed", "crashed"]);
  assert.equal((await getJobByKey(wf, "cr")).status, "queued", "requeued for the next Attempt at the next Tier");

  const to = await getAttempt(wf, "to-a1");
  assert.equal(to.status, "in_progress", "attempt run closes a timed-out Attempt, as `timed_out` is set");
  assert.equal(to.metadata.timed_out, true);

  const sent = readdirSync(queueDir).filter((n) => n.endsWith(".json")).map((n) => JSON.parse(readFileSync(join(queueDir, n), "utf8")));
  assert.deepEqual(sent.map((m) => [m.kind, m.attempt]), [["attempt-retry", "cr-a1"]]);
  assert.match(sent[0]!.text, /cr-a2 will start at Tier H2/);
  assert.equal((await listAttempts(wf)).length, 3);

  // all-merged is claimed in write-once Workflow metadata: the first claim wins, the second is told so.
  assert.equal(await real.ledger.phase(), "dispatch");
  assert.equal(await real.ledger.claimAllMerged(), true);
  assert.equal(await real.ledger.claimAllMerged(), false);
});
