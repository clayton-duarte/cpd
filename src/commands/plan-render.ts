import { resolve } from "node:path";
import { parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import type { Result } from "../lib/output.ts";
import { renderPlan } from "../render/plan.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("plan render", argv, { wf: { type: "string" }, out: { type: "string" } });
  const { path, fingerprint } = await renderPlan(resolveWorkflowId(values.wf), values.out === undefined ? undefined : resolve(values.out));
  return { ok: true, path, fingerprint };
}
