import { readdirSync, readFileSync, unlinkSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { join } from "node:path";
import { z } from "zod";
import { agentGet } from "../herdr/agents.ts";
import { closePane } from "../herdr/panes.ts";
import { logEvent } from "../ledger/audit.ts";
import { listIssues, metadataArgs, readLedger, withLedger, workflowIssue } from "../ledger/bd.ts";
import { getAttempt, listAttempts, failAttempt, recordAttemptFacts } from "../ledger/attempts.ts";
import { expireEscalation, recordAnswer, recordEscalation } from "../ledger/escalations.ts";
import { cancelJob, listJobs, readyJobs, type Job } from "../ledger/jobs.ts";
import { getPhase, readWorkflow, writeJsonAtomic, type Phase } from "../ledger/workflow.ts";
import { loadConfig, type Config } from "../lib/config.ts";
import { CpdError, failure, refused } from "../lib/errors.ts";
import { run } from "../lib/exec.ts";
import { pidAlive, withLock } from "../lib/lock.ts";
import type { Result } from "../lib/output.ts";
import { escalationsDir, ledgerLockPath, slotsDir, watchPath } from "../lib/paths.ts";
import { expireOrphans, expireQuestion, readAnswer, readQuestion } from "../queue/escalation-files.ts";
import { notifyLead, type NotifyInput } from "../queue/lead-queue.ts";
import type { Clock } from "./attempt-run.ts";
import { CPD_ENTRY, dispatchOne, realDispatchDeps } from "./dispatch-one.ts";
import {
  attemptOfEscalation,
  checkLiveness,
  mirrorEscalations,
  tell,
  type EscalationFiles,
  type EscalationPorts,
  type LedgerEscalation,
  type LivenessPorts,
  type Log,
  type Report,
} from "./liveness.ts";
import { scheduleReady, type SchedulePorts } from "./schedule.ts";

// Contract §11 `cpd watch`: one live instance per Workflow, then every `tickSeconds` the steps
// 1 Escalations · 2 Liveness · 3 Scheduling · 4 Completion · 5 PR · 6 Exit. The steps live in liveness.ts and
// schedule.ts; this file owns the loop, the single-instance claim, error handling, completion and the PR poll.

// --- watch.json: one live instance (§4) ---

const WatchFile = z.strictObject({ pid: z.number().int().positive(), startedAt: z.iso.datetime(), heartbeatAt: z.iso.datetime() });
export type WatchFile = z.infer<typeof WatchFile>;

export function readWatchFile(wf: string): WatchFile | null {
  const path = watchPath(wf);
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw failure("watch_state_unreadable", `cannot read ${path}: ${(e as Error).message}`, { path });
  }
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw failure("watch_state_invalid", `${path} is not valid JSON; delete it if no cpd watch is running`, { path });
  }
  const parsed = WatchFile.safeParse(raw);
  if (!parsed.success) throw failure("watch_state_invalid", `${path} does not match the schema; delete it if no cpd watch is running`, { path, issues: parsed.error.issues });
  return parsed.data;
}

/** Who this watch is and when its predecessor counts as dead. Injectable so the claim is testable. */
export type WatchIdentity = { pid: number; now(): number; alive(pid: number): boolean; staleAfterMs: number };

/** A holder is stale when its pid is dead, or when its heartbeat is older than this (a reused pid): five minutes or ten ticks. */
export const staleAfterMs = (config: Pick<Config, "tickSeconds">): number => Math.max(5 * 60_000, 10 * config.tickSeconds * 1000);

export const realIdentity = (config: Pick<Config, "tickSeconds">): WatchIdentity => ({
  pid: process.pid,
  now: () => Date.now(),
  alive: pidAlive,
  staleAfterMs: staleAfterMs(config),
});

const iso = (ms: number): string => new Date(ms).toISOString();

/** Take `watch.json`, or refuse `watch_running` (exit 3) when a live watch holds it. A stale holder is taken over and returned. */
export function claimWatch(wf: string, id: WatchIdentity): Promise<{ tookOver: WatchFile | null }> {
  return withLock(ledgerLockPath(wf), "watch claim", () => {
    const held = readWatchFile(wf);
    const now = id.now();
    if (held !== null && id.alive(held.pid) && now - Date.parse(held.heartbeatAt) < id.staleAfterMs) {
      throw refused("watch_running", `cpd watch is already running for ${wf} (pid ${held.pid}, heartbeat ${held.heartbeatAt})`, { wf, pid: held.pid, heartbeatAt: held.heartbeatAt });
    }
    writeJsonAtomic(watchPath(wf), { pid: id.pid, startedAt: iso(now), heartbeatAt: iso(now) } satisfies WatchFile);
    return { tookOver: held };
  });
}

/** Refresh the heartbeat. `watch_superseded` when `watch.json` is no longer ours (a takeover): this watch must stop. */
export function heartbeatWatch(wf: string, id: WatchIdentity): Promise<void> {
  return withLock(ledgerLockPath(wf), "watch heartbeat", () => {
    const held = readWatchFile(wf);
    if (held === null || held.pid !== id.pid) {
      throw failure("watch_superseded", `cpd watch ${id.pid} no longer holds watch.json (held by ${held === null ? "nobody" : held.pid})`, { wf, pid: id.pid, holder: held?.pid ?? null });
    }
    writeJsonAtomic(watchPath(wf), { ...held, heartbeatAt: iso(id.now()) } satisfies WatchFile);
  });
}

/** Remove `watch.json` if it is ours. */
export function releaseWatch(wf: string, id: WatchIdentity): Promise<void> {
  return withLock(ledgerLockPath(wf), "watch release", () => {
    if (readWatchFile(wf)?.pid === id.pid) unlinkSync(watchPath(wf));
  });
}

// --- errors: log, tell the Lead at a bounded rate, stop only for corruption ---

/** The same error is sent to the Lead at most this often. */
export const ERROR_REPEAT_MS = 10 * 60_000;

/** Config or Ledger corruption: nothing sensible can happen until a person fixes it. `watch_superseded` is the only other way the loop ends. */
const FATAL: ReadonlySet<string> = new Set([
  "config_missing",
  "config_unreadable",
  "config_invalid",
  "ledger_invalid",
  "workflow_not_found",
  "workflow_unreadable",
  "workflow_invalid",
  "watch_state_invalid",
  "watch_superseded",
]);

export const isFatal = (e: unknown): boolean => e instanceof CpdError && FATAL.has(e.code);

const errorParts = (e: unknown): { code: string; message: string } =>
  e instanceof CpdError ? { code: e.code, message: e.message } : { code: "unexpected", message: e instanceof Error ? e.message : String(e) };

const clip = (s: string, n = 600): string => (s.length > n ? `${s.slice(0, n)}…` : s);

export type ReporterPorts = { wf: string; clock: Clock; log: Log; queue: { notify(input: NotifyInput): unknown } };

function sendError(p: ReporterPorts, step: string, e: unknown, fatal: boolean): void {
  const { code, message } = errorParts(e);
  const text = fatal
    ? `cpd watch for Workflow ${p.wf} stopped at step ${step}: ${clip(message)} (${code}). Nothing more is scheduled until it is fixed. Fix the cause, then restart with: cpd watch --wf ${p.wf}`
    : `cpd watch hit an error at step ${step}: ${clip(message)} (${code}). It keeps running and will not repeat this message for 10 minutes. Details are in logs/cpd.jsonl; the Workflow state: cpd workflow show --wf ${p.wf}`;
  try {
    p.queue.notify({ kind: "error", text, data: { step, code, message, fatal } });
  } catch (n) {
    p.log("error", "notify_failed", { step, code, message: n instanceof Error ? n.message : String(n) });
  }
}

/** The loop is ending because of `e`: log it and tell the Lead once (except when a newer watch took over, which is not an error). */
export function announceFatal(p: ReporterPorts, step: string, e: unknown): void {
  const { code, message } = errorParts(e);
  p.log("error", "watch_stopped", { step, code, message });
  if (code !== "watch_superseded") sendError(p, step, e, true);
}

/** The `Report` the steps use. Fatal errors are rethrown (the loop announces them); the rest are logged and sent once per distinct error per 10 minutes. */
export function makeReporter(p: ReporterPorts): Report {
  const sent = new Map<string, number>();
  return (step, e) => {
    if (isFatal(e)) throw e;
    const { code, message } = errorParts(e);
    p.log("warn", "watch_error", { step, code, message });
    const now = p.clock.now();
    for (const [k, at] of sent) if (now - at >= ERROR_REPEAT_MS) sent.delete(k);
    const key = `${step}|${code}|${message}`;
    if (sent.has(key)) return;
    sent.set(key, now);
    sendError(p, step, e, false);
  };
}

// --- the ports of one watch ---

export type WatchDeps = LivenessPorts &
  EscalationPorts &
  SchedulePorts & {
    config: { attemptTimeoutMinutes: number; retryCap: number; tickSeconds: number; prPollSeconds: number };
    ledger: { phase(): Promise<Phase>; claimAllMerged(): Promise<boolean> };
    /** `cpd pr poll --wf <wf>` in a subprocess. A result that is not ok comes back as a value; a process that could not run throws. */
    pr: { poll(): Promise<{ ok: true } | { ok: false; code: string; message: string }> };
    /** Refresh `watch.json`; throws `watch_superseded` when another watch took it over. */
    heartbeat(): Promise<void>;
  };

/** What the loop remembers between ticks. */
export type WatchState = { lastPrPollAt: number | null; allMergedHandled: boolean };
export const newState = (): WatchState => ({ lastPrPollAt: null, allMergedHandled: false });

// --- step 4: completion ---

/** Every Job is Merged or cancelled (§10 `all-merged`), and at least one is Merged. A `failed` Job still waits for the Lead's retry or cancel. */
export function allMerged(jobs: readonly Pick<Job, "status">[]): boolean {
  return jobs.some((j) => j.status === "closed") && jobs.every((j) => j.status === "closed" || j.status === "cancelled");
}

/** Send `all-merged` once: the write-once Workflow metadata `all_merged_at` is claimed first. */
async function announceCompletion(d: WatchDeps, s: WatchState, jobs: readonly Job[]): Promise<boolean> {
  if (!allMerged(jobs)) return false;
  const claimed = await d.ledger.claimAllMerged();
  s.allMergedHandled = true;
  if (!claimed) return false;
  const merged = jobs.filter((j) => j.status === "closed").map((j) => j.key);
  const cancelled = jobs.filter((j) => j.status === "cancelled").map((j) => j.key);
  d.log("info", "all_merged", { merged, cancelled });
  tell(d, {
    kind: "all-merged",
    text:
      `Every Job of Workflow ${d.wf} is Merged or cancelled (merged: ${merged.join(", ")}${cancelled.length > 0 ? `; cancelled: ${cancelled.join(", ")}` : ""}). ` +
      `Open the Pull Request with: cpd pr open --wf ${d.wf}`,
    data: { merged, cancelled },
  });
  return true;
}

// --- step 5: the PR ---

/** In phase `pr`, poll at the first tick and then every `everyMs`. */
export const prPollDue = (phase: Phase, now: number, lastPollAt: number | null, everyMs: number): boolean =>
  phase === "pr" && (lastPollAt === null || now - lastPollAt >= everyMs);

async function pollPr(d: WatchDeps): Promise<void> {
  const r = await d.pr.poll();
  if (r.ok) {
    d.log("info", "pr_polled");
    return;
  }
  d.report("pr poll", failure("pr_poll_failed", `cpd pr poll failed: ${r.code}: ${r.message}`, { code: r.code }));
}

// --- the tick and the loop ---

export type TickResult = {
  done: boolean;
  started: string[];
  cancelled: string[];
  timedOut: string[];
  crashed: string[];
  allMerged: boolean;
  polledPr: boolean;
};

/** A step that fails is reported and the tick goes on with the next one. Fatal errors propagate. */
async function step<T>(d: WatchDeps, name: string, fn: () => Promise<T>): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    d.report(name, e);
    return undefined;
  }
}

/** One pass of steps 1 to 6. `done` when the phase is `done`. */
export async function tick(d: WatchDeps, s: WatchState): Promise<TickResult> {
  const phase = await d.ledger.phase();
  const r: TickResult = { done: phase === "done", started: [], cancelled: [], timedOut: [], crashed: [], allMerged: false, polledPr: false };
  if (r.done) return r;

  await step(d, "escalations", () => mirrorEscalations(d));
  const live = await step(d, "liveness", () => checkLiveness(d));
  if (live) Object.assign(r, { cancelled: live.cancelled, timedOut: live.timedOut, crashed: live.crashed });
  const scheduled = await step(d, "schedule", () => scheduleReady(d));
  if (scheduled) r.started = scheduled.started.map((x) => x.attempt);
  // Completion reads no Ledger of its own (a `bd` call takes about a second): it uses the Jobs liveness read, and only
  // when this tick changed none of them. A tick that did try again on the next one.
  const unchanged = live !== undefined && scheduled !== undefined && scheduled.started.length + live.cancelled.length + live.crashed.length + live.jobsCancelled.length === 0;
  if (phase === "dispatch" && !s.allMergedHandled && unchanged) r.allMerged = (await step(d, "completion", () => announceCompletion(d, s, live.jobs))) ?? false;
  if (prPollDue(phase, d.clock.now(), s.lastPrPollAt, d.config.prPollSeconds * 1000)) {
    s.lastPrPollAt = d.clock.now(); // a failed poll waits for the next interval too
    r.polledPr = true;
    await step(d, "pr", () => pollPr(d));
    r.done = (await d.ledger.phase()) === "done";
  }
  return r;
}

/**
 * Tick every `tickSeconds` until the phase is `done`. A failing tick is reported and the loop goes on; config or Ledger
 * corruption (and a takeover) ends it, after the Lead has been told.
 */
export async function runLoop(d: WatchDeps, s: WatchState = newState()): Promise<{ ticks: number }> {
  let ticks = 0;
  try {
    for (;;) {
      try {
        await d.heartbeat();
        const r = await tick(d, s);
        ticks++;
        if (r.done) return { ticks };
      } catch (e) {
        d.report("tick", e);
      }
      await d.clock.sleep(d.config.tickSeconds * 1000);
    }
  } catch (e) {
    announceFatal(d, "watch", e);
    throw e;
  }
}

// --- real ports ---

/** Events go to `logs/cpd.jsonl` (§2) and, for whoever is looking at the watch pane, to stderr. */
function makeLog(wf: string): Log {
  return (level, event, data) => {
    logEvent(wf, level, event, data);
    process.stderr.write(`${new Date().toISOString()} ${level} ${event}${data === undefined ? "" : ` ${JSON.stringify(data)}`}\n`);
  };
}

const Slot = z.looseObject({ wf: z.string(), attempt: z.string(), pid: z.number().int().positive() });

/**
 * Whether the process that holds the slot of Attempt `attempt` of Workflow `wf` is alive: the slot's pid is the `attempt run`
 * that took it over (§9). Slots are machine-wide and Attempt ids (`<job-key>-a<n>`) are only unique per Workflow, so the slot's `wf` must match too.
 */
export function runAlive(wf: string, attempt: string): boolean {
  let names: string[];
  try {
    names = readdirSync(slotsDir());
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw e;
  }
  for (const name of names.filter((n) => /^slot-[0-9]+\.json$/.test(n))) {
    const path = join(slotsDir(), name);
    let text: string;
    try {
      text = readFileSync(path, "utf8");
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === "ENOENT") continue; // released since the listing
      throw e;
    }
    let raw: unknown;
    try {
      raw = JSON.parse(text);
    } catch {
      throw failure("slot_corrupt", `slot file is not valid JSON: ${path}`, { path });
    }
    const slot = Slot.safeParse(raw);
    if (!slot.success) throw failure("slot_corrupt", `slot file does not match the schema: ${path}`, { path, issues: slot.error.issues });
    if (slot.data.wf === wf && slot.data.attempt === attempt) return pidAlive(slot.data.pid);
  }
  return false;
}

const EscalationMeta = z.looseObject({
  esc_id: z.string(),
  asked_at: z.string(),
  answered_at: z.string().optional(),
  expired_at: z.string().optional(),
});

/** Escalation issues as `LedgerEscalation`: a closed one ends at its answer, its expiry or its close time. */
async function ledgerEscalations(wf: string): Promise<LedgerEscalation[]> {
  return (await listIssues(readLedger(wf), "escalation")).map((issue) => {
    const meta = EscalationMeta.safeParse(issue.metadata ?? {});
    if (!meta.success) throw failure("ledger_invalid", `Escalation issue ${issue.id} does not have the expected shape`, { id: issue.id, issues: meta.error.issues });
    const open = issue.status !== "closed";
    const from = Date.parse(meta.data.asked_at);
    const to = open ? null : Date.parse(meta.data.answered_at ?? meta.data.expired_at ?? issue.closed_at ?? "");
    if (Number.isNaN(from) || (to !== null && Number.isNaN(to))) {
      throw failure("ledger_invalid", `Escalation issue ${issue.id} has no valid times`, { id: issue.id, askedAt: meta.data.asked_at });
    }
    return { id: meta.data.esc_id, attempt: attemptOfEscalation(meta.data.esc_id), open, from, to };
  });
}

const ESCALATION_ID = /^[a-z][a-z0-9-]{0,31}-a[1-9][0-9]*-q[1-9][0-9]*$/;

/** The ids of `escalations/*.q.json`, `*.a.json` and `*.expired.json`. An `.expired.json` is never a question. */
export function listEscalationFiles(wf: string): EscalationFiles {
  let names: string[];
  try {
    names = readdirSync(escalationsDir(wf));
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return { questions: [], answers: [], expired: [] };
    throw e;
  }
  const ids = (suffix: string) =>
    names
      .filter((n) => n.endsWith(suffix) && ESCALATION_ID.test(n.slice(0, -suffix.length)))
      .map((n) => n.slice(0, -suffix.length))
      .sort();
  return { questions: ids(".q.json"), answers: ids(".a.json"), expired: ids(".expired.json") };
}

/** Workflow metadata `all_merged_at`, write-once: true only for the call that wrote it. */
async function claimAllMerged(wf: string): Promise<boolean> {
  return withLedger(wf, "all-merged", async (l) => {
    const issue = await workflowIssue(l);
    if ("all_merged_at" in (issue.metadata ?? {})) return false;
    await l.write(["update", issue.id, ...metadataArgs(issue.metadata, { all_merged_at: new Date().toISOString() }, true)]);
    return true;
  });
}

const PollOutput = z.union([
  z.looseObject({ ok: z.literal(true) }),
  z.looseObject({ ok: z.literal(false), error: z.looseObject({ code: z.string(), message: z.string() }) }),
]);

/** `cpd pr poll` as a process, so `watch` does not depend on the PR module (§11 step 5). */
async function pollInSubprocess(wf: string): Promise<{ ok: true } | { ok: false; code: string; message: string }> {
  const r = await run(process.execPath, [CPD_ENTRY, "pr", "poll", "--wf", wf], { cwd: process.cwd(), env: process.env, timeoutMs: 180_000 });
  let raw: unknown;
  try {
    raw = JSON.parse(r.stdout);
  } catch {
    throw failure("bad_tool_output", `cpd pr poll ${r.signal ? `was killed by ${r.signal}` : `exited ${r.code}`} without a JSON result`, {
      stdout: r.stdout.slice(-2000),
      stderr: r.stderr.slice(-2000),
    });
  }
  const out = PollOutput.safeParse(raw);
  if (!out.success) throw failure("bad_tool_output", "cpd pr poll printed an unexpected result", { issues: out.error.issues });
  return out.data.ok ? { ok: true } : { ok: false, code: out.data.error.code, message: out.data.error.message };
}

/** The real ports for Workflow `wf`. Needs the pi catalog (for the Tier ladder), so it can fail at startup. */
export async function realWatchDeps(wf: string, config: Config, identity: WatchIdentity): Promise<WatchDeps> {
  const dispatchDeps = await realDispatchDeps(wf);
  const log = makeLog(wf);
  const clock: Clock = { now: () => Date.now(), sleep: (ms) => sleep(ms) };
  const queue = { notify: (input: NotifyInput) => notifyLead(wf, input) };
  const ports: Omit<WatchDeps, "report"> = {
    wf,
    config,
    catalog: dispatchDeps.catalog,
    clock,
    log,
    queue,
    ledger: {
      phase: () => getPhase(wf),
      jobs: () => listJobs(wf),
      attempts: () => listAttempts(wf),
      attempt: (id) => getAttempt(wf, id),
      escalations: () => ledgerEscalations(wf),
      markTimedOut: (id) => recordAttemptFacts(wf, id, { timed_out: true }),
      failAttempt: (id, cause, o) => failAttempt(wf, id, cause, o),
      cancelJob: (key, reason) => cancelJob(wf, key, reason),
      recordEscalation: (attempt, o) => recordEscalation(wf, attempt, o),
      recordAnswer: (id, o) => recordAnswer(wf, id, o),
      expireEscalation: (id, o) => expireEscalation(wf, id, o),
      claimAllMerged: () => claimAllMerged(wf),
    },
    herdr: {
      async agentAlive(name) {
        return (await agentGet(name)) !== null;
      },
      async closePane(id) {
        try {
          await closePane(id);
        } catch (e) {
          if (!(e instanceof CpdError && e.code === "herdr_pane_not_found")) throw e;
        }
      },
    },
    runAlive: (id) => runAlive(wf, id),
    expireQuestion: (id) => expireQuestion(wf, id),
    files: {
      list: () => listEscalationFiles(wf),
      question: (id) => readQuestion(wf, id),
      answer: (id) => readAnswer(wf, id),
      expireOrphans: () => expireOrphans(wf),
    },
    ready: () => readyJobs(wf),
    dispatch: (key) => dispatchOne(wf, key, dispatchDeps),
    pr: { poll: () => pollInSubprocess(wf) },
    heartbeat: () => heartbeatWatch(wf, identity),
  };
  return { ...ports, report: makeReporter(ports) };
}

/**
 * `cpd watch --wf <wf>`: claim `watch.json`, build the ports, loop until the phase is `done`.
 * Startup failures are told to the Lead too: the watch pane has no one reading it.
 */
export async function runWatch(wf: string): Promise<Result> {
  readWorkflow(wf); // workflow_not_found (exit 4) before anything is written
  const base: ReporterPorts = {
    wf,
    clock: { now: () => Date.now(), sleep: (ms) => sleep(ms) },
    log: makeLog(wf),
    queue: { notify: (input) => notifyLead(wf, input) },
  };
  let config: Config;
  try {
    config = loadConfig();
  } catch (e) {
    announceFatal(base, "startup", e);
    throw e;
  }
  const identity = realIdentity(config);
  const { tookOver } = await claimWatch(wf, identity);
  if (tookOver !== null) base.log("warn", "watch_takeover", { previousPid: tookOver.pid, previousHeartbeatAt: tookOver.heartbeatAt });
  try {
    let deps: WatchDeps;
    try {
      deps = await realWatchDeps(wf, config, identity);
    } catch (e) {
      announceFatal(base, "startup", e);
      throw e;
    }
    base.log("info", "watch_started", { pid: identity.pid, tickSeconds: config.tickSeconds });
    const { ticks } = await runLoop(deps);
    base.log("info", "watch_done", { ticks });
    return { ok: true, wf, phase: "done", ticks };
  } finally {
    try {
      await releaseWatch(wf, identity);
    } catch (e) {
      base.log("warn", "watch_release_failed", { message: e instanceof Error ? e.message : String(e) });
    }
  }
}
