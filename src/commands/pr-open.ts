import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";
import { openPr } from "../pr/open.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("pr open", argv, { wf: { type: "string" } });
  return { ok: true, pr: await openPr(resolveWorkflowId(values.wf)) };
}
