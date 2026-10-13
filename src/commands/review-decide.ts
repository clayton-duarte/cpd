import { decideReview, realDeps } from "../dispatch/attempt-run.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertAttemptId } from "../lib/paths.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand(
    "review decide",
    argv,
    { wf: { type: "string" }, approve: { type: "boolean" }, reject: { type: "boolean" }, feedback: { type: "string" } },
    1,
  );
  const id = positionals[0];
  if (id === undefined) throw usage("usage", "cpd review decide: pass the Attempt id");
  assertAttemptId(id);
  if (values.approve === values.reject) throw usage("usage", "cpd review decide: pass exactly one of --approve and --reject");
  if (values.feedback === undefined || values.feedback.trim() === "") throw usage("usage", "cpd review decide: --feedback <text> is required and must not be blank");
  const wf = resolveWorkflowId(values.wf);
  const result = await decideReview(await realDeps(wf, id, null), { verdict: values.approve ? "approve" : "reject", feedback: values.feedback });
  return { ok: true, attempt: id, ...result };
}
