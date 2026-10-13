import { cancelJob, jobView } from "../ledger/jobs.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("job cancel", argv, { wf: { type: "string" }, reason: { type: "string" } }, 1);
  if (positionals[0] === undefined) throw usage("usage", "cpd job cancel: pass the Job key");
  if (values.reason === undefined) throw usage("usage", "cpd job cancel: --reason is required");
  const r = await cancelJob(resolveWorkflowId(values.wf), positionals[0], values.reason);
  return { ok: true, job: jobView(r.job), cancelled: r.cancelled, cancelRequested: !r.cancelled, alreadyRequested: r.alreadyRequested };
}
