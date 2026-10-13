import { attemptProvenance, findAttempt } from "../ledger/attempts.ts";
import { listIssues, readLedger } from "../ledger/bd.ts";
import { BdComments } from "../ledger/schema.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("attempt show", argv, { wf: { type: "string" } }, 1);
  if (positionals[0] === undefined) throw usage("usage", "cpd attempt show: pass the Attempt id");
  const wf = resolveWorkflowId(values.wf);
  const l = readLedger(wf);
  const { attempt } = await findAttempt(l, positionals[0]);
  const comments = (await l.read(["comments", attempt.issue], BdComments)).map((c) => ({ author: c.author, text: c.text, at: c.created_at }));
  const escalations = (await listIssues(l, "escalation", ["--parent", attempt.issue])).map((e) => ({
    id: typeof e.metadata?.esc_id === "string" ? e.metadata.esc_id : e.id,
    status: e.status,
    question: e.description ?? "",
  }));
  const provenance = await attemptProvenance(wf, attempt.id);
  return {
    ok: true,
    attempt,
    comments,
    escalations,
    provenance,
  };
}
