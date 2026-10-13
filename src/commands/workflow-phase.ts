import { usage } from "../lib/errors.ts";
import { refused } from "../lib/errors.ts";
import type { Result } from "../lib/output.ts";
import { getPhase, parseCommand, parsePhase, resolveWorkflowId, setPhase } from "../ledger/workflow.ts";

/** The only manual transition is context → plan; every other one belongs to `plan lock`, `pr open` and `pr poll`. */
export async function run(argv: string[]): Promise<Result> {
  const { values, positionals } = parseCommand("workflow phase", argv, { wf: { type: "string" } }, 1);
  if (positionals[0] === undefined) throw usage("usage", "cpd workflow phase: pass the target phase");
  const to = parsePhase(positionals[0]);
  const wf = resolveWorkflowId(values.wf);
  const from = await getPhase(wf);
  if (to !== "plan" || from !== "context") {
    throw refused("illegal_transition", `phase ${from} cannot be moved to ${to} with this command (only context → plan)`, { from, to });
  }
  await setPhase(wf, to);
  return { ok: true, phase: to };
}
