import { listTamper } from "../ledger/audit.ts";
import { JOB_STATUSES, listJobsWithAttempts } from "../ledger/jobs.ts";
import { getPhase, parseCommand, readWorkflow, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("workflow show", argv, { wf: { type: "string" } });
  const wf = resolveWorkflowId(values.wf);
  const workflow = readWorkflow(wf);
  const phase = await getPhase(wf);
  const jobs = await listJobsWithAttempts(wf);
  const counts = Object.fromEntries(JOB_STATUSES.map((s) => [s, jobs.filter((j) => j.status === s).length]));
  return {
    ok: true,
    workflow,
    phase,
    jobs: jobs.map((j) => ({ key: j.key, title: j.title, status: j.status, tier: j.tier, needs: j.needs, attempts: j.attempts })),
    counts,
    tamper: await listTamper(wf),
  };
}
