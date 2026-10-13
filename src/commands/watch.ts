import { runWatch } from "../dispatch/watch.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("watch", argv, { wf: { type: "string" } });
  return runWatch(resolveWorkflowId(values.wf));
}
