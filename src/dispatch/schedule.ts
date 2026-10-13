import type { Job } from "../ledger/jobs.ts";
import { CpdError } from "../lib/errors.ts";
import type { Dispatched } from "./dispatch-one.ts";
import type { Log, Report } from "./liveness.ts";

// Contract §11 `watch` step 3: start ready Jobs, in creation order, while a slot is free. The machine-wide cap is
// the slots themselves (§9): `dispatchOne` takes one before it starts anything, so two Workflows cannot exceed it.

export type SchedulePorts = {
  /** `queued` Jobs whose Dependencies are all Merged, in creation order. */
  ready(): Promise<Job[]>;
  /** `dispatchOne` for this Workflow. */
  dispatch(jobKey: string): Promise<Dispatched>;
  log: Log;
  report: Report;
};

export type ScheduleResult = { started: Dispatched[]; noSlot: boolean };

/** Refusals that only mean somebody else got to the Job first (`cpd job dispatch` by hand): logged, not reported. */
const RACES: ReadonlySet<string> = new Set(["cas_lost", "job_not_ready", "attempt_open"]);

/**
 * Dispatch each ready Job until the first `no_slot`. Any other refusal (`ladder_exhausted`, `locked_job_tampered`,
 * `lead_not_attached`, …) is about that Job or Workflow: it is reported and the next Job is tried. A tool failure
 * (exit 1) is thrown: the tick's scheduling stops and the caller reports it.
 */
export async function scheduleReady(d: SchedulePorts): Promise<ScheduleResult> {
  const result: ScheduleResult = { started: [], noSlot: false };
  for (const job of await d.ready()) {
    try {
      const r = await d.dispatch(job.key);
      result.started.push(r);
      d.log("info", "attempt_dispatched", { job: job.key, attempt: r.attempt, slot: r.slot, pid: r.pid, tier: r.tier });
    } catch (e) {
      if (!(e instanceof CpdError) || e.exitCode !== 3) throw e;
      if (e.code === "no_slot") {
        result.noSlot = true;
        d.log("info", "no_slot", { job: job.key });
        break;
      }
      if (RACES.has(e.code)) d.log("info", "dispatch_skipped", { job: job.key, code: e.code });
      else d.report(`schedule ${job.key}`, e);
    }
  }
  return result;
}
