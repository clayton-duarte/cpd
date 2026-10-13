import { test } from "node:test";
import assert from "node:assert/strict";
import type { Attempt } from "../../src/ledger/attempts.ts";
import type { Job } from "../../src/ledger/jobs.ts";
import { CpdError, failure, refused } from "../../src/lib/errors.ts";
import type { NotifyInput } from "../../src/queue/lead-queue.ts";
import {
  activeMs,
  checkLiveness,
  judge,
  mirrorEscalations,
  planExpiry,
  planMirror,
  tracked,
  type EscalationFiles,
  type EscalationPorts,
  type LedgerEscalation,
  type LivenessPorts,
  type Observed,
  type Span,
  type Tracked,
} from "../../src/dispatch/liveness.ts";
import { CATALOG } from "./fakes.ts";

const MIN = 60_000;
const T0 = Date.parse("2026-01-01T00:00:00.000Z");
const at = (minutes: number) => T0 + minutes * MIN;

// ---------- pure: active time ----------

test("active time = elapsed minus the time Escalations were open", () => {
  assert.equal(activeMs(T0, at(20), []), 20 * MIN);
  assert.equal(activeMs(T0, at(20), [{ from: at(5), to: at(15) }]), 10 * MIN, "a 10-minute question is not Worker time");
  assert.equal(activeMs(T0, at(20), [{ from: at(15), to: null }]), 15 * MIN, "a question still open stops the clock");
});

test("overlapping, out-of-range and zero-length spans are merged and clipped to the Attempt's life", () => {
  const spans: Span[] = [
    { from: at(-10), to: at(2) }, // started before the Attempt: only 2 minutes count
    { from: at(5), to: at(10) },
    { from: at(8), to: at(12) }, // overlaps the previous one: 7 minutes together
    { from: at(30), to: at(40) }, // in the future
    { from: at(14), to: at(14) },
  ];
  assert.equal(activeMs(T0, at(20), spans), 20 * MIN - (2 + 7) * MIN);
  assert.equal(activeMs(T0, at(5), [{ from: T0, to: null }]), 0, "never negative");
});

// ---------- pure: snapshot and decision ----------

const job = (over: Partial<Job> = {}): Job => ({
  id: "bd-1", key: "api", title: "Api", status: "running", tier: "S3", reviewTier: "S4", goal: "g", context: "c",
  touches: ["a"], acceptance: ["a"], outOfScope: [], needs: [], closeReason: null, metadata: { key: "api", tier: "S3" }, ...over,
});
const attempt = (over: Partial<Attempt> & { metadata?: Record<string, unknown> } = {}): Attempt => {
  const metadata = { number: 1, tier: "S3", started_at: new Date(T0).toISOString(), agent: "cagent", worker_pane: "w1:p2", ...over.metadata };
  return {
    id: "api-a1", issue: "bd-a1", job: "api", jobIssue: "bd-1", number: 1, tier: "S3", status: "in_progress", outcome: null, cause: null,
    worktree: "/wt/api-a1", branch: "cpd/demo--api-a1", agent: "cagent", sessionPath: null, startedAt: new Date(T0).toISOString(),
    submittedAt: null, endedAt: null, closeReason: null, ...over, metadata,
  };
};
const esc = (over: Partial<LedgerEscalation> = {}): LedgerEscalation => ({ id: "api-a1-q1", attempt: "api-a1", open: false, from: at(5), to: at(15), ...over });

const view = (a: Attempt = attempt(), j: Job = job(), e: LedgerEscalation[] = []): Tracked => tracked([a], [j], e)[0]!;
const LIMIT = 30 * MIN;
const alive: Observed = { runAlive: true, agent: "unchecked" };

test("tracked: only open Attempts, with the Job's cancel request, the timed_out flag and the Escalation spans", () => {
  const closed = attempt({ id: "api-a0", number: 0 as never, status: "closed" });
  const [t, ...rest] = tracked([closed, attempt({ metadata: { timed_out: true } })], [job({ metadata: { key: "api", cancel_requested: "x" } })], [esc(), esc({ attempt: "other-a1" })]);
  assert.equal(rest.length, 0);
  assert.deepEqual([t!.cancelRequested, t!.timedOut, t!.workerPane, t!.startedAt], [true, true, "w1:p2", T0]);
  assert.deepEqual(t!.spans, [{ from: at(5), to: at(15) }]);
});

test("tracked: an Attempt without a valid started_at, or without a Job, is ledger_invalid", () => {
  assert.throws(() => tracked([attempt({ startedAt: null })], [job()], []), (e: unknown) => e instanceof CpdError && e.code === "ledger_invalid");
  assert.throws(() => tracked([attempt()], [], []), (e: unknown) => e instanceof CpdError && e.code === "ledger_invalid");
});

test("judge: the decision table", () => {
  const w = view();
  assert.deepEqual(judge(w, alive, at(5), LIMIT), { kind: "none" });
  assert.deepEqual(judge(w, alive, at(31), LIMIT), { kind: "timeout" }, "past the limit");
  assert.deepEqual(judge(w, alive, at(30), LIMIT), { kind: "none" }, "exactly at the limit is not past it");
  assert.deepEqual(judge(w, { runAlive: false, agent: "gone" }, at(5), LIMIT), { kind: "crash", cause: "crashed" });
  assert.deepEqual(judge(w, { runAlive: true, agent: "gone" }, at(5), LIMIT), { kind: "none" }, "attempt run is alive: it decides what a missing Worker means");
  assert.deepEqual(judge(w, { runAlive: false, agent: "alive" }, at(5), LIMIT), { kind: "none" }, "the Worker is there");

  const cancelled = view(attempt(), job({ metadata: { key: "api", cancel_requested: "x" } }));
  assert.deepEqual(judge(cancelled, alive, at(1), LIMIT), { kind: "cancel" });
  assert.deepEqual(judge(cancelled, { runAlive: false, agent: "gone" }, at(1), LIMIT), { kind: "cancel" }, "a cancel wins over a crash");

  const timedOut = view(attempt({ metadata: { timed_out: true } }));
  assert.deepEqual(judge(timedOut, { runAlive: true, agent: "alive" }, at(99), LIMIT), { kind: "close_pane" }, "the pane is still open: close it again");
  assert.deepEqual(judge(timedOut, { runAlive: true, agent: "gone" }, at(99), LIMIT), { kind: "none" });
  assert.deepEqual(judge(timedOut, { runAlive: false, agent: "gone" }, at(99), LIMIT), { kind: "crash", cause: "timed_out" }, "nobody is left to close a timed-out Attempt");
});

test("judge: an Escalation open for 10 minutes extends the timeout by 10 minutes", () => {
  const asked = [esc({ from: at(5), to: at(15) })];
  assert.deepEqual(judge(view(attempt(), job(), asked), alive, at(39), LIMIT), { kind: "none" }, "39 minutes elapsed, 29 active");
  assert.deepEqual(judge(view(attempt(), job(), asked), alive, at(41), LIMIT), { kind: "timeout" }, "41 elapsed, 31 active");
  assert.deepEqual(judge(view(attempt(), job(), []), alive, at(31), LIMIT), { kind: "timeout" }, "without the question, 31 minutes is a timeout");
  const open = [esc({ from: at(20), to: null, open: true })];
  assert.deepEqual(judge(view(attempt(), job(), open), alive, at(600), LIMIT), { kind: "none" }, "an open question stops the clock");
});

test("judge: an in_review Attempt is not timed out or crashed; its cancel waits for a live attempt run (it may be merging)", () => {
  const review = attempt({ status: "in_review", submittedAt: new Date(T0).toISOString() });
  assert.deepEqual(judge(view(review), { runAlive: false, agent: "unchecked" }, at(600), LIMIT), { kind: "none" });
  const cancelled = view(review, job({ status: "in_review", metadata: { key: "api", cancel_requested: "x" } }));
  assert.deepEqual(judge(cancelled, { runAlive: true, agent: "unchecked" }, at(1), LIMIT), { kind: "none" });
  assert.deepEqual(judge(cancelled, { runAlive: false, agent: "unchecked" }, at(1), LIMIT), { kind: "cancel" });
});

// ---------- the step, against a fake world ----------

type World = {
  now: number;
  jobs: Job[];
  attempts: Attempt[];
  escalations: LedgerEscalation[];
  /** Attempts whose `attempt run` is alive. */
  running: Set<string>;
  /** herdr agents that exist. */
  agents: Set<string>;
  events: string[];
  messages: NotifyInput[];
  reports: string[];
  failClosePane?: Error;
};

function world(over: Partial<World> = {}): World {
  return { now: at(1), jobs: [job()], attempts: [attempt()], escalations: [], running: new Set(), agents: new Set(["cagent"]), events: [], messages: [], reports: [], ...over };
}

function ports(w: World, retryCap = 2): LivenessPorts {
  const attemptOf = (id: string) => w.attempts.find((a) => a.id === id)!;
  const jobOf = (key: string) => w.jobs.find((j) => j.key === key)!;
  return {
    wf: "demo",
    config: { attemptTimeoutMinutes: 30, retryCap },
    catalog: CATALOG,
    clock: { now: () => w.now, sleep: async () => {} },
    log: () => {},
    report: (step, e) => {
      if (e instanceof CpdError && e.code === "ledger_invalid") throw e;
      w.reports.push(`${step}: ${(e as Error).message}`);
    },
    ledger: {
      jobs: async () => structuredClone(w.jobs),
      attempts: async () => structuredClone(w.attempts),
      attempt: async (id) => structuredClone(attemptOf(id)),
      escalations: async () => structuredClone(w.escalations),
      async markTimedOut(id) {
        w.events.push(`ledger.timed_out ${id}`);
        attemptOf(id).metadata.timed_out = true;
      },
      async failAttempt(id, cause, o) {
        const a = attemptOf(id);
        if (w.escalations.some((e) => e.attempt === id && e.open)) throw refused("escalation_open", `Attempt ${id} has an open Escalation`);
        w.events.push(`ledger.fail ${id} ${cause} requeue=${o.requeue}`);
        a.status = "closed";
        jobOf(a.job).status = cause === "cancelled" ? "cancelled" : o.requeue ? "queued" : "failed";
      },
      async cancelJob(key, reason) {
        w.events.push(`ledger.cancelJob ${key} ${reason}`);
        jobOf(key).status = "cancelled";
      },
      async expireEscalation(id, o) {
        w.events.push(`ledger.expire ${id} ${o.reason}`);
        const e = w.escalations.find((x) => x.id === id)!;
        e.open = false;
        e.to = w.now;
      },
    },
    herdr: {
      async agentAlive(name) {
        w.events.push(`herdr.agentAlive ${name}`);
        return w.agents.has(name);
      },
      async closePane(id) {
        w.events.push(`herdr.closePane ${id}`);
        if (w.failClosePane) throw w.failClosePane;
        w.agents.delete("cagent");
      },
    },
    runAlive: (id) => w.running.has(id),
    expireQuestion: (id) => void w.events.push(`file.expire ${id}`),
    queue: { notify: (m) => void w.messages.push(m) },
  };
}

test("a timeout writes timed_out first, then closes the Worker pane", async () => {
  const w = world({ now: at(31), running: new Set(["api-a1"]) });
  const r = await checkLiveness(ports(w));
  assert.deepEqual(r.timedOut, ["api-a1"]);
  assert.deepEqual(w.events, ["ledger.timed_out api-a1", "herdr.closePane w1:p2"]);
  assert.equal(w.attempts[0]!.metadata.timed_out, true);
  assert.equal(w.attempts[0]!.status, "in_progress", "closing the Attempt is attempt run's job: it reads timed_out");
  assert.deepEqual(w.messages, [], "attempt run sends job-failed");
});

test("below the limit nothing happens, and no herdr call is made while attempt run is alive", async () => {
  const w = world({ now: at(29), running: new Set(["api-a1"]) });
  await checkLiveness(ports(w));
  assert.deepEqual(w.events, []);
});

test("a timeout whose pane close fails is retried on the next tick, while the Worker is still there", async () => {
  const w = world({ now: at(31), running: new Set(["api-a1"]), failClosePane: failure("herdr_busy", "herdr is busy") });
  await checkLiveness(ports(w));
  assert.equal(w.reports.length, 1, "reported, not thrown");
  assert.equal(w.attempts[0]!.metadata.timed_out, true);
  w.failClosePane = undefined;
  w.events.length = 0;
  w.now = at(32);
  await checkLiveness(ports(w));
  assert.deepEqual(w.events, ["herdr.agentAlive cagent", "herdr.closePane w1:p2"], "no second timed_out write (it is write-once)");
  w.events.length = 0;
  await checkLiveness(ports(w));
  assert.deepEqual(w.events, ["herdr.agentAlive cagent"], "the Worker is gone: nothing more to close");
});

test("an Escalation that stayed open 10 minutes extends the timeout by 10 minutes (the step, end to end)", async () => {
  const asked = [esc({ from: at(5), to: at(15) })];
  const w = world({ now: at(39), running: new Set(["api-a1"]), escalations: asked });
  assert.deepEqual((await checkLiveness(ports(w))).timedOut, []);
  w.now = at(41);
  assert.deepEqual((await checkLiveness(ports(w))).timedOut, ["api-a1"]);
});

test("a crash (Worker agent gone, attempt run not alive) fails the Attempt as crashed, requeues it via the ladder and tells the Lead", async () => {
  const w = world({ agents: new Set(), running: new Set() });
  const r = await checkLiveness(ports(w));
  assert.deepEqual(r.crashed, ["api-a1"]);
  assert.deepEqual(w.events, ["herdr.agentAlive cagent", "herdr.closePane w1:p2", "ledger.fail api-a1 crashed requeue=true"]);
  assert.equal(w.jobs[0]!.status, "queued");
  assert.equal(w.messages.length, 1);
  assert.equal(w.messages[0]!.kind, "attempt-retry");
  assert.equal(w.messages[0]!.attempt, "api-a1");
  assert.match(w.messages[0]!.text, /api-a2 will start at Tier S4/);
  assert.match(w.messages[0]!.text, /cpd attempt show api-a1 --wf demo/);
});

test("a crash with no Worker ever started (no agent recorded) and no attempt run is a crash too, without asking herdr", async () => {
  const w = world({ attempts: [attempt({ agent: null })] });
  delete w.attempts[0]!.metadata.agent;
  delete w.attempts[0]!.metadata.worker_pane;
  const r = await checkLiveness(ports(w));
  assert.deepEqual(r.crashed, ["api-a1"]);
  assert.deepEqual(w.events, ["ledger.fail api-a1 crashed requeue=true"], "no pane to close, no agent to look up");
});

test("a crash past the retry cap fails the Job and sends job-failed instead", async () => {
  const w = world({
    agents: new Set(),
    jobs: [job({ metadata: { key: "api", tier: "S3" } })],
    attempts: [attempt({ id: "api-a3", number: 3, metadata: { number: 3 } })],
  });
  await checkLiveness(ports(w, 2));
  assert.deepEqual(w.events.slice(-1), ["ledger.fail api-a3 crashed requeue=false"]);
  assert.equal(w.jobs[0]!.status, "failed");
  assert.equal(w.messages[0]!.kind, "job-failed");
  assert.match(w.messages[0]!.text, /retry cap is reached/);
  assert.match(w.messages[0]!.text, /cpd job retry api --tier <T> --wf demo/);
});

test("a crash while attempt run is alive is left to attempt run", async () => {
  const w = world({ agents: new Set(), running: new Set(["api-a1"]) });
  assert.deepEqual((await checkLiveness(ports(w))).crashed, []);
  assert.deepEqual(w.events, []);
});

test("a timed-out Attempt that nobody closed (attempt run died) is failed as timed_out with no retry", async () => {
  const w = world({ agents: new Set(), attempts: [attempt({ metadata: { timed_out: true } })] });
  await checkLiveness(ports(w));
  assert.deepEqual(w.events.slice(-1), ["ledger.fail api-a1 timed_out requeue=false"]);
  assert.equal(w.messages[0]!.kind, "job-failed");
  assert.match(w.messages[0]!.text, /30-minute active-time limit/);
});

test("cancel_requested stops the Attempt (pane closed), closes it as cancelled and cancels the Job", async () => {
  const w = world({ running: new Set(["api-a1"]), jobs: [job({ metadata: { key: "api", cancel_requested: "2026-01-01T00:00:30Z", cancel_reason: "no longer needed" } })] });
  const r = await checkLiveness(ports(w));
  assert.deepEqual(r.cancelled, ["api-a1"]);
  assert.deepEqual(w.events, ["herdr.closePane w1:p2", "ledger.fail api-a1 cancelled requeue=false"]);
  assert.equal(w.attempts[0]!.status, "closed");
  assert.equal(w.jobs[0]!.status, "cancelled");
  assert.deepEqual(w.messages, []);
});

test("cancelling an Attempt with an open Escalation first expires its question, or the Attempt could not be closed", async () => {
  const w = world({
    running: new Set(["api-a1"]),
    jobs: [job({ metadata: { key: "api", cancel_requested: "x", cancel_reason: "r" } })],
    escalations: [esc({ open: true, to: null })],
  });
  await checkLiveness(ports(w));
  assert.deepEqual(w.events, ["herdr.closePane w1:p2", "file.expire api-a1-q1", "ledger.expire api-a1-q1 orphan", "ledger.fail api-a1 cancelled requeue=false"]);
  assert.equal(w.jobs[0]!.status, "cancelled");
});

test("an in_review Attempt is cancelled only once its attempt run is gone", async () => {
  const review = attempt({ status: "in_review", submittedAt: new Date(at(5)).toISOString() });
  const requested = job({ status: "in_review", metadata: { key: "api", cancel_requested: "x", cancel_reason: "r" } });
  const w = world({ attempts: [review], jobs: [requested], running: new Set(["api-a1"]) });
  assert.deepEqual((await checkLiveness(ports(w))).cancelled, [], "attempt run is reviewing or merging");
  w.running.clear();
  assert.deepEqual((await checkLiveness(ports(w))).cancelled, ["api-a1"], "waiting for cpd review decide: nobody else will finish it");
  assert.equal(w.jobs[0]!.status, "cancelled");
});

test("a requested cancel whose Attempt already ended is finished on the Job (it would otherwise be started again)", async () => {
  const w = world({
    attempts: [attempt({ status: "closed", outcome: "failed" })],
    jobs: [job({ status: "queued", metadata: { key: "api", cancel_requested: "x", cancel_reason: "changed my mind" } })],
  });
  await checkLiveness(ports(w));
  assert.deepEqual(w.events, ["ledger.cancelJob api changed my mind"]);
  assert.equal(w.jobs[0]!.status, "cancelled");
  await checkLiveness(ports(w));
  assert.equal(w.events.length, 1, "a cancelled Job is left alone");
});

test("an Attempt that changed between the snapshot and the action (the Worker just submitted) is left alone this tick", async () => {
  const w = world({ now: at(31), running: new Set(["api-a1"]) });
  const p = ports(w);
  const read = p.ledger.attempts;
  p.ledger.attempts = async () => {
    const a = await read();
    w.attempts[0]!.status = "in_review"; // the Worker submitted right after the snapshot was taken
    w.attempts[0]!.submittedAt = new Date(at(30)).toISOString();
    return a;
  };
  const r = await checkLiveness(p);
  assert.deepEqual([r.timedOut, w.events], [[], []]);
});

test("one Attempt failing is reported and does not stop the others", async () => {
  const a2 = attempt({ id: "web-a1", job: "web", issue: "bd-a2", agent: "cweb", metadata: { agent: "cweb", worker_pane: "w1:p3" } });
  const w = world({
    now: at(31),
    attempts: [attempt(), a2],
    jobs: [job(), job({ id: "bd-2", key: "web", metadata: { key: "web", tier: "S3" } })],
    running: new Set(["api-a1", "web-a1"]),
    agents: new Set(["cagent", "cweb"]),
  });
  const p = ports(w);
  const mark = p.ledger.markTimedOut;
  p.ledger.markTimedOut = async (id) => {
    if (id === "api-a1") throw failure("bd_failed", "bd is down");
    return mark(id);
  };
  const r = await checkLiveness(p);
  assert.deepEqual(r.timedOut, ["web-a1"]);
  assert.equal(w.reports.length, 1);
  assert.match(w.reports[0]!, /liveness api-a1: bd is down/);
});

test("Ledger corruption is not swallowed", async () => {
  const w = world({ attempts: [attempt({ startedAt: null })] });
  await assert.rejects(checkLiveness(ports(w)), (e: unknown) => e instanceof CpdError && e.code === "ledger_invalid");
});

// ---------- Escalations ----------

const files = (over: Partial<EscalationFiles> = {}): EscalationFiles => ({ questions: [], answers: [], expired: [], ...over });

test("planMirror: new questions are recorded; answers only for Escalations that are not closed", () => {
  const known = new Map<string, "open" | "closed">([["api-a1-q1", "open"], ["api-a1-q2", "closed"]]);
  assert.deepEqual(planMirror(files({ questions: ["api-a1-q1", "api-a1-q2", "api-a1-q3"], answers: ["api-a1-q1", "api-a1-q2"] }), known), [
    { kind: "record", id: "api-a1-q3" },
    { kind: "answer", id: "api-a1-q1" },
  ]);
  assert.deepEqual(planMirror(files({ questions: ["api-a1-q4"], answers: ["api-a1-q4"] }), new Map()), [
    { kind: "record", id: "api-a1-q4" },
    { kind: "answer", id: "api-a1-q4" },
  ], "asked and answered between two ticks: recorded, then answered");
  assert.deepEqual(planMirror(files({ answers: ["api-a1-q9"] }), new Map()), [], "an answer with no question anywhere is not ours");
});

test("planExpiry: only expired questions whose Escalation is open in the Ledger", () => {
  const known = new Map<string, "open" | "closed">([["api-a1-q1", "open"], ["api-a1-q2", "closed"]]);
  assert.deepEqual(planExpiry(["api-a1-q1", "api-a1-q2", "api-a1-q3"], known), [{ kind: "expire", id: "api-a1-q1" }]);
});

function escalationWorld(f: EscalationFiles, ledger: LedgerEscalation[] = [], orphans: string[] = []) {
  const log: string[] = [];
  const state = { files: f, ledger: [...ledger], reports: [] as string[] };
  const p: EscalationPorts = {
    wf: "demo",
    clock: { now: () => at(10), sleep: async () => {} },
    log: () => {},
    report: (step, e) => void state.reports.push(`${step}: ${(e as Error).message}`),
    ledger: {
      escalations: async () => structuredClone(state.ledger),
      async recordEscalation(attempt, o) {
        log.push(`record ${attempt} ${o.escId} ${o.askedAt} ${o.question}`);
        state.ledger.push({ id: o.escId, attempt, open: true, from: Date.parse(o.askedAt), to: null });
      },
      async recordAnswer(id, o) {
        log.push(`answer ${id} ${o.answeredAt} ${o.answer}`);
        state.ledger.find((e) => e.id === id)!.open = false;
      },
      async expireEscalation(id, o) {
        log.push(`expire ${id} ${o.reason}`);
        state.ledger.find((e) => e.id === id)!.open = false;
      },
    },
    files: {
      list: () => structuredClone(state.files),
      question: (id) => ({ v: 1, id, wf: "demo", attempt: id.slice(0, id.lastIndexOf("-q")), question: `why ${id}?`, askedAt: new Date(at(2)).toISOString(), askerPid: 1 }),
      answer: (id) => ({ v: 1, id, answer: `because ${id}`, answeredAt: new Date(at(4)).toISOString() }),
      expireOrphans() {
        log.push("expireOrphans");
        for (const id of orphans) {
          state.files.questions = state.files.questions.filter((q) => q !== id);
          state.files.expired.push(id);
        }
        return orphans;
      },
    },
  };
  return { p, log, state };
}

test("mirrorEscalations records a new question and a new answer in the Ledger", async () => {
  const { p, log } = escalationWorld(files({ questions: ["api-a1-q1", "api-a1-q2"], answers: ["api-a1-q1"] }));
  await mirrorEscalations(p);
  assert.deepEqual(log, [
    `record api-a1 api-a1-q1 ${new Date(at(2)).toISOString()} why api-a1-q1?`,
    `record api-a1 api-a1-q2 ${new Date(at(2)).toISOString()} why api-a1-q2?`,
    `answer api-a1-q1 ${new Date(at(4)).toISOString()} because api-a1-q1`,
    "expireOrphans",
  ]);
  log.length = 0;
  await mirrorEscalations(p);
  assert.deepEqual(log, ["expireOrphans"], "idempotent: everything is mirrored already");
});

test("an orphaned question (asker dead) is recorded first, then expired as orphan", async () => {
  const { p, log, state } = escalationWorld(files({ questions: ["api-a1-q1"] }), [], ["api-a1-q1"]);
  await mirrorEscalations(p);
  assert.deepEqual(log.map((l) => l.split(" ").slice(0, 3).join(" ")), ["record api-a1 api-a1-q1", "expireOrphans", "expire api-a1-q1 orphan"]);
  assert.equal(state.ledger[0]!.open, false);
});

test("an <id>.expired.json (ask timeout) closes its open Escalation as timeout, and is never read as a question", async () => {
  const { p, log } = escalationWorld(files({ expired: ["api-a1-q1"] }), [{ id: "api-a1-q1", attempt: "api-a1", open: true, from: at(2), to: null }]);
  await mirrorEscalations(p);
  assert.deepEqual(log, ["expireOrphans", "expire api-a1-q1 timeout"]);
  log.length = 0;
  await mirrorEscalations(p);
  assert.deepEqual(log, ["expireOrphans"], "already closed: nothing to do");
});

test("an expired question that was never recorded cannot block an Attempt and is left alone", async () => {
  const { p, log } = escalationWorld(files({ expired: ["api-a1-q7"] }));
  await mirrorEscalations(p);
  assert.deepEqual(log, ["expireOrphans"]);
});

test("with no escalation file at all, nothing is read from the Ledger (a bd call takes about a second)", async () => {
  const { p, log } = escalationWorld(files());
  p.ledger.escalations = async () => {
    throw new Error("the Ledger must not be read");
  };
  await mirrorEscalations(p);
  assert.deepEqual(log, []);
});

test("with no open Attempt, liveness does not read the Escalations either", async () => {
  const w = world({ attempts: [attempt({ status: "closed", outcome: "failed" })], jobs: [job({ status: "queued" })] });
  const p = ports(w);
  p.ledger.escalations = async () => {
    throw new Error("the Ledger must not be read");
  };
  assert.deepEqual((await checkLiveness(p)).crashed, []);
});

test("a question that cannot be recorded is reported once and its answer is not tried either", async () => {
  const { p, log, state } = escalationWorld(files({ questions: ["api-a1-q1"], answers: ["api-a1-q1"] }));
  p.ledger.recordEscalation = async () => {
    throw refused("illegal_transition", "cannot record an Escalation: Attempt api-a1 is closed");
  };
  await mirrorEscalations(p);
  assert.equal(state.reports.length, 1);
  assert.deepEqual(log, ["expireOrphans"]);
});
