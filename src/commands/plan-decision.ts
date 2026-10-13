import { readFileSync } from "node:fs";
import { recordPlanDecision, type DecisionState } from "../ledger/plan.ts";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("plan decision", argv, {
    wf: { type: "string" },
    "review-id": { type: "string" },
    pending: { type: "boolean" },
    approved: { type: "boolean" },
    rejected: { type: "boolean" },
    "feedback-file": { type: "string" },
  });
  if (values["review-id"] === undefined) throw usage("usage", "cpd plan decision: --review-id is required");
  const states = (["pending", "approved", "rejected"] as const).filter((s) => values[s]);
  if (states.length !== 1) throw usage("usage", "cpd plan decision: pass exactly one of --pending, --approved, --rejected");
  const state: DecisionState = states[0]!;
  let feedback: string | undefined;
  if (values["feedback-file"] !== undefined) {
    try {
      feedback = readFileSync(values["feedback-file"], "utf8");
    } catch (e) {
      throw usage("feedback_unreadable", `cannot read feedback file: ${(e as Error).message}`, { path: values["feedback-file"] });
    }
  }
  const record = await recordPlanDecision(resolveWorkflowId(values.wf), values["review-id"], state, feedback);
  return { ok: true, reviewId: values["review-id"], status: record.status, fingerprint: record.fingerprint };
}
