import { spawn, type ChildProcess, type SpawnOptions } from "node:child_process";
import { closeSync, mkdirSync, openSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { attemptCount, failAttempt, startAttempt, type Attempt } from "../ledger/attempts.ts";
import { getJobByKey, type Job } from "../ledger/jobs.ts";
import { readWorkflow } from "../ledger/workflow.ts";
import { loadConfig } from "../lib/config.ts";
import { failure, refused } from "../lib/errors.ts";
import { assertJobKey, attemptId, attemptLogPath, cpdHome, logsDir } from "../lib/paths.ts";
import { acquireSlot, releaseSlot, setSlotPid } from "../lib/slots.ts";
import { attemptTier, formatTier, loadCatalog, type Catalog, type ParsedTier } from "../lib/tiers.ts";

// Contract §11 `job dispatch`: the one step `cpd job dispatch` and `cpd watch` both run for a ready Job.

const RETRY_TIER = /^retry_([1-9][0-9]*)_tier$/;

/** The latest Attempt number ≤ `n` that started a run: an explicit `job retry` Tier (`retry_<m>_tier`), else Attempt 1. */
function runStart(job: Pick<Job, "metadata">, n: number): { number: number; tier: string | null } {
  let start = { number: 1, tier: null as string | null };
  for (const [key, value] of Object.entries(job.metadata)) {
    const m = RETRY_TIER.exec(key);
    if (m && typeof value === "string" && Number(m[1]) <= n && Number(m[1]) > start.number) start = { number: Number(m[1]), tier: value };
  }
  return start;
}

/**
 * The Tier of Attempt `n` (§7): the Lead's explicit `retry_<n>_tier` if there is one, otherwise the ladder applied to
 * the Tier that started the current run (the Job's Tier, or the latest explicit retry Tier before `n`). Null when the ladder runs out.
 */
export function tierForAttempt(catalog: Catalog, job: Pick<Job, "tier" | "metadata">, n: number): ParsedTier | null {
  const start = runStart(job, n);
  return attemptTier(catalog, start.tier ?? job.tier, n - start.number + 1);
}

/** Attempts of the current automatic run up to and including Attempt `n`: the retry cap counts these (§7). */
export const automaticAttemptCount = (job: Pick<Job, "metadata">, n: number): number => n - runStart(job, n).number + 1;

export type DispatchDeps = {
  concurrency: number;
  catalog: Catalog;
  /** Refuses (`lead_not_attached`, `lead_worktree_missing`) before anything is taken, since `attempt run` needs both. */
  assertReady(): void;
  readJob(key: string): Promise<Job>;
  attemptCount(key: string): Promise<number>;
  acquireSlot(who: { attempt: string }): Promise<number | null>;
  releaseSlot(index: number, attempt: string): Promise<unknown>;
  setSlotPid(index: number, attempt: string, pid: number): Promise<void>;
  /** Compare-and-swap `queued → running` at `tier`; null when the race was lost. */
  startAttempt(key: string, tier: string): Promise<Attempt | null>;
  /** Close an Attempt whose process could not be started; the Job becomes `failed`. */
  failAttempt(attempt: string, cause: string): Promise<unknown>;
  /** Spawn `cpd attempt run <attempt> --wf <wf> --slot <slot>` detached, its output going to `logs/attempt-<id>.log`. */
  spawnRun(attempt: string, slot: number): { pid: number };
};

export type Dispatched = { attempt: string; slot: number; pid: number; tier: string };

/** The `bin/cpd.mjs` of this checkout. */
export const CPD_ENTRY = fileURLToPath(new URL("../../bin/cpd.mjs", import.meta.url));

/** The argv (after `node`) that starts one Attempt run. */
export const attemptRunArgs = (wf: string, attempt: string, slot: number): string[] => [CPD_ENTRY, "attempt", "run", attempt, "--wf", wf, "--slot", String(slot)];

/** `spawn`, as far as `spawnDetached` uses it (a seam for tests). */
export type Spawn = (cmd: string, args: string[], opts: SpawnOptions) => Pick<ChildProcess, "pid" | "on" | "unref">;

/** Spawn `cpd attempt run` detached: it outlives this process, with stdout and stderr appended to `logs/attempt-<id>.log`. */
export function spawnDetached(wf: string, attempt: string, slot: number, spawnFn: Spawn = spawn): { pid: number } {
  mkdirSync(logsDir(wf), { recursive: true });
  const log = openSync(attemptLogPath(wf, attempt), "a");
  try {
    const child = spawnFn(process.execPath, attemptRunArgs(wf, attempt, slot), {
      detached: true,
      stdio: ["ignore", log, log],
      cwd: cpdHome(),
      env: process.env,
    });
    child.on("error", () => {}); // a failed spawn is reported through the missing pid
    child.unref();
    if (child.pid === undefined) throw failure("spawn_failed", `could not spawn cpd attempt run for ${attempt}`, { attempt });
    return { pid: child.pid };
  } finally {
    closeSync(log);
  }
}

/** Real ports for Workflow `wf`. Build once and reuse (`cpd watch` calls `dispatchOne` for every ready Job). */
export async function realDispatchDeps(wf: string): Promise<DispatchDeps> {
  const config = loadConfig();
  return {
    concurrency: config.concurrency,
    catalog: await loadCatalog(config),
    assertReady() {
      const w = readWorkflow(wf);
      if (w.lead === null) throw refused("lead_not_attached", `Workflow ${wf} has no attached Lead`, { wf });
      if (w.leadWorktree === null) throw refused("lead_worktree_missing", `Workflow ${wf} has no Lead worktree (plan lock has not run)`, { wf });
    },
    readJob: (key) => getJobByKey(wf, key),
    attemptCount: (key) => attemptCount(wf, key),
    acquireSlot: (who) => acquireSlot(config.concurrency, { wf, attempt: who.attempt }),
    releaseSlot,
    setSlotPid,
    startAttempt: (key, tier) => startAttempt(wf, key, { tier }),
    failAttempt: (attempt, cause) => failAttempt(wf, attempt, cause, { requeue: false }),
    spawnRun: (attempt, slot) => spawnDetached(wf, attempt, slot),
  };
}

/**
 * Start one ready Job now. Order: slot, then the CAS `startAttempt` at the Attempt's Tier, then a detached `attempt run`
 * that takes the slot over (`setSlotPid`). The slot is released on any failure after it was acquired.
 * Refusals (exit 3): `no_slot`, `cas_lost`, `job_not_ready`, `ladder_exhausted`, `lead_not_attached`.
 */
export async function dispatchOne(wf: string, jobKey: string, deps?: DispatchDeps): Promise<Dispatched> {
  assertJobKey(jobKey);
  const d = deps ?? (await realDispatchDeps(wf));
  d.assertReady();
  const job = await d.readJob(jobKey);
  const number = (await d.attemptCount(jobKey)) + 1;
  const id = attemptId(jobKey, number);
  const tier = tierForAttempt(d.catalog, job, number);
  if (tier === null) throw refused("ladder_exhausted", `no Tier left for Attempt ${id}`, { attempt: id });

  const slot = await d.acquireSlot({ attempt: id });
  if (slot === null) throw refused("no_slot", `no free concurrency slot (concurrency ${d.concurrency})`, { concurrency: d.concurrency });
  try {
    const started = await d.startAttempt(jobKey, formatTier(tier));
    if (started === null) throw refused("cas_lost", `Job ${jobKey} was not queued any more: another dispatcher won`, { job: jobKey });
    let pid: number;
    try {
      if (started.id !== id) throw failure("attempt_id_mismatch", `started ${started.id} but the slot was taken for ${id}`, { started: started.id, expected: id });
      pid = d.spawnRun(id, slot).pid;
    } catch (e) {
      await d.failAttempt(started.id, "dispatch_failed"); // do not leave a running Job with no process behind it
      throw e;
    }
    await d.setSlotPid(slot, id, pid);
    return { attempt: id, slot, pid, tier: formatTier(tier) };
  } catch (e) {
    await d.releaseSlot(slot, id);
    throw e;
  }
}
