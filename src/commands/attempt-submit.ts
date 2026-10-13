import { submitAttempt } from "../dispatch/attempt-submit.ts";
import { markSubmitted, findAttempt } from "../ledger/attempts.ts";
import { readLedger } from "../ledger/bd.ts";
import { parseCommand, readWorkflow, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertAttemptId } from "../lib/paths.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("attempt submit", argv, { wf: { type: "string" } }, 1);
  const id = positionals[0];
  if (id === undefined) throw usage("usage", "cpd attempt submit: pass the Attempt id");
  assertAttemptId(id);
  const wf = resolveWorkflowId(values.wf);
  const { attempt } = await findAttempt(readLedger(wf), id);
  const r = await submitAttempt(process.cwd(), {
    wf,
    attempt,
    leadBranch: readWorkflow(wf).leadBranch,
    markSubmitted: (sha) => markSubmitted(wf, id, sha),
  });
  return { ok: true, submitted: true, next: r.next, attempt: id, commit: r.commit };
}
