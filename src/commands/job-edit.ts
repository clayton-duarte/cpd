import { editJob, jobView, loadTierContext, readSpecFile, splitKeys } from "../ledger/jobs.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand(
    "job edit",
    argv,
    {
      wf: { type: "string" },
      title: { type: "string" },
      tier: { type: "string" },
      spec: { type: "string" },
      needs: { type: "string" },
      "review-tier": { type: "string" },
    },
    1,
  );
  const key = positionals[0];
  if (key === undefined) throw usage("usage", "cpd job edit: pass the Job key");
  const wf = resolveWorkflowId(values.wf);
  const { catalog } = await loadTierContext();
  const job = await editJob(
    wf,
    key,
    {
      title: values.title,
      tier: values.tier,
      reviewTier: values["review-tier"],
      spec: values.spec === undefined ? undefined : readSpecFile(values.spec),
      needs: values.needs === undefined ? undefined : splitKeys(values.needs),
    },
    catalog,
  );
  return { ok: true, job: jobView(job) };
}
