import { runWithSlot } from "../dispatch/attempt-run.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertAttemptId } from "../lib/paths.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("attempt run", argv, { wf: { type: "string" }, slot: { type: "string" } }, 1);
  const id = positionals[0];
  if (id === undefined) throw usage("usage", "cpd attempt run: pass the Attempt id");
  assertAttemptId(id);
  if (values.slot === undefined || !/^[0-9]+$/.test(values.slot)) {
    throw usage("usage", "cpd attempt run: --slot <i> is required (the slot `cpd job dispatch` or `cpd watch` acquired), a non-negative integer");
  }
  const result = await runWithSlot(resolveWorkflowId(values.wf), id, Number(values.slot));
  return { ok: true, attempt: id, ...result };
}
