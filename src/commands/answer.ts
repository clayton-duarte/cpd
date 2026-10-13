import { parseArgs } from "node:util";
import { refused, usage } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { assertEscalationId, assertWorkflowId } from "../lib/paths.ts";
import { notifyLead } from "../queue/lead-queue.ts";
import { readAnswer, readQuestion, writeAnswer } from "../queue/escalation-files.ts";

export async function run(argv: string[]): Promise<Result> {
  let parsed;
  try {
    parsed = parseArgs({ args: argv, allowPositionals: true, options: { wf: { type: "string" } } });
  } catch (e) {
    throw usage("usage", (e as Error).message);
  }
  const [id, answer, ...extra] = parsed.positionals;
  if (id === undefined || answer === undefined || extra.length > 0) {
    throw usage("usage", "usage: cpd answer <esc-id> <answer> --wf <wf>");
  }
  assertEscalationId(id);
  const wf = parsed.values.wf ?? process.env.CPD_WF;
  if (!wf) throw usage("workflow_required", "--wf <id> (or CPD_WF) is required");
  assertWorkflowId(wf);

  if (readAnswer(wf, id)) throw refused("escalation_answered", `escalation ${id} is already answered`, { id, wf });
  const q = readQuestion(wf, id);
  if (!q) throw refused("escalation_missing", `no open escalation ${id}`, { id, wf });
  if (!writeAnswer(wf, { v: 1, id, answer, answeredAt: new Date().toISOString() })) {
    throw refused("escalation_answered", `escalation ${id} is already answered`, { id, wf });
  }
  notifyLead(wf, {
    kind: "escalation-answered",
    attempt: q.attempt,
    data: { id },
    text: `Escalation ${id} from ${q.attempt} was answered; the Worker is unblocked.`,
  });
  return { ok: true, id, answered: true };
}
