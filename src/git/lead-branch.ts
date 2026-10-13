import { failure } from "../lib/errors.ts";
import { branchExists, realpath } from "./git.ts";
import { switchCreateArgs, switchExistingArgs, wtSwitch } from "./wt.ts";

/**
 * Contract §12 / t3 F1: the Lead Branch lives in its own wt worktree, never in the primary checkout.
 * Idempotent: an existing branch returns its worktree path via `wt switch <branch>` (`action: existing`).
 */
export async function createLeadBranch(repo: string, leadBranch: string, base: string): Promise<{ path: string }> {
  const exists = await branchExists(repo, leadBranch);
  const out = await wtSwitch(repo, exists ? switchExistingArgs(leadBranch) : switchCreateArgs(leadBranch, base));
  if (out.branch !== leadBranch) {
    throw failure("bad_tool_output", `wt switch returned branch ${out.branch}, expected ${leadBranch}`, { out });
  }
  return { path: realpath(out.path) };
}
