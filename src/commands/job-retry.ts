import { jobView, loadTierContext, retryJob } from "../ledger/jobs.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("job retry", argv, { wf: { type: "string" }, tier: { type: "string" } }, 1);
  if (positionals[0] === undefined) throw usage("usage", "cpd job retry: pass the Job key");
  if (values.tier === undefined) throw usage("usage", "cpd job retry: --tier is required");
  const { catalog } = await loadTierContext();
  const r = await retryJob(resolveWorkflowId(values.wf), positionals[0], values.tier, catalog);
  return { ok: true, job: jobView(r.job), attempt: r.attempt, tier: r.tier };
}
