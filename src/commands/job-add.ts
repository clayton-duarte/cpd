import { addJob, jobView, loadTierContext, readSpecFile, splitKeys } from "../ledger/jobs.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("job add", argv, {
    wf: { type: "string" },
    key: { type: "string" },
    title: { type: "string" },
    tier: { type: "string" },
    spec: { type: "string" },
    needs: { type: "string" },
    "review-tier": { type: "string" },
  });
  for (const f of ["key", "title", "tier", "spec"] as const) {
    if (values[f] === undefined) throw usage("usage", `cpd job add: --${f} is required`);
  }
  const wf = resolveWorkflowId(values.wf);
  const spec = readSpecFile(values.spec!);
  const { catalog, defaultReviewTier } = await loadTierContext();
  const { job, fingerprint } = await addJob(
    wf,
    {
      key: values.key!,
      title: values.title!,
      tier: values.tier!,
      reviewTier: values["review-tier"] ?? defaultReviewTier,
      spec,
      needs: splitKeys(values.needs ?? ""),
    },
    catalog,
  );
  return { ok: true, job: jobView(job), fingerprint };
}
