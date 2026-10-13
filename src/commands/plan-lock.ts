import { planLock } from "../dispatch/plan-lock.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("plan lock", argv, { wf: { type: "string" }, "no-watch": { type: "boolean" } });
  const r = await planLock(resolveWorkflowId(values.wf), { watch: values["no-watch"] !== true });
  return { ok: true, leadBranch: r.leadBranch, leadWorktree: r.leadWorktree, watchPane: r.watchPane };
}
