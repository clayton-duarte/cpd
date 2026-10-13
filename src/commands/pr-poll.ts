import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";
import { pollPr } from "../pr/poll.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("pr poll", argv, { wf: { type: "string" } });
  const r = await pollPr(resolveWorkflowId(values.wf));
  return { ok: true, changes: r.changes, merged: r.merged };
}
