import { listAttempts } from "../ledger/attempts.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("attempt list", argv, { wf: { type: "string" }, status: { type: "string" } });
  const attempts = await listAttempts(resolveWorkflowId(values.wf), { status: values.status });
  return {
    ok: true,
    attempts: attempts.map((a) => ({ id: a.id, job: a.job, number: a.number, tier: a.tier, status: a.status, outcome: a.outcome, cause: a.cause })),
  };
}
