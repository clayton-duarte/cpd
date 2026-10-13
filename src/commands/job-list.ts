import { jobView, listJobs } from "../ledger/jobs.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("job list", argv, { wf: { type: "string" }, status: { type: "string" } });
  const jobs = await listJobs(resolveWorkflowId(values.wf), { status: values.status });
  return { ok: true, jobs: jobs.map(jobView) };
}
