import { dispatchOne } from "../dispatch/dispatch-one.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("job dispatch", argv, { wf: { type: "string" } }, 1);
  if (positionals[0] === undefined) throw usage("usage", "cpd job dispatch: pass the Job key");
  const r = await dispatchOne(resolveWorkflowId(values.wf), positionals[0]);
  return { ok: true, attempt: r.attempt, slot: r.slot, pid: r.pid, tier: r.tier };
}
