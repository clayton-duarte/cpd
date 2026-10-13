import { jobView, showJob } from "../ledger/jobs.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("job show", argv, { wf: { type: "string" } }, 1);
  if (positionals[0] === undefined) throw usage("usage", "cpd job show: pass the Job key");
  const s = await showJob(resolveWorkflowId(values.wf), positionals[0]);
  const { job } = s;
  return {
    ok: true,
    job: {
      ...jobView(job),
      goal: job.goal,
      context: job.context,
      touches: job.touches,
      acceptance: job.acceptance,
      outOfScope: job.outOfScope,
      closeReason: job.closeReason,
      metadata: job.metadata,
    },
    comments: s.comments,
    attempts: s.attempts,
  };
}
