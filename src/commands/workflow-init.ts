import type { Result } from "../lib/output.ts";
import { initWorkflow, parseCommand, resolveWorkflowId } from "../ledger/workflow.ts";
import { usage } from "../lib/errors.ts";

export async function run(argv: string[]): Promise<Result> {
  const { values } = parseCommand("workflow init", argv, {
    wf: { type: "string" },
    repo: { type: "string" },
    title: { type: "string" },
    base: { type: "string" },
  });
  if (values.repo === undefined) throw usage("repo_required", "pass --repo <path>");
  if (values.title === undefined) throw usage("title_required", "pass --title <text>");
  const workflow = await initWorkflow({ wf: resolveWorkflowId(values.wf), repo: values.repo, title: values.title, base: values.base });
  return { ok: true, workflow };
}
