import { createLeadBranch } from "../git/lead-branch.ts";
import { assertNoControlChars } from "../herdr/herdr.ts";
import { splitPane } from "../herdr/panes.ts";
import { logEvent } from "../ledger/audit.ts";
import { lockPlan } from "../ledger/plan.ts";
import { getPhase, readWorkflow, writeWorkflow } from "../ledger/workflow.ts";
import { loadConfig } from "../lib/config.ts";
import { CpdError, failure, refused } from "../lib/errors.ts";
import { run } from "../lib/exec.ts";

// Contract §11 `plan lock`: the Ledger part (`lockPlan`), the Lead Branch worktree, and the `cpd watch` pane.

export type PlanLocked = { leadBranch: string; leadWorktree: string; watchPane: string | null };

/**
 * `herdr pane run <pane> <command>`: types `command` into the pane's shell. It prints nothing on success (probed on herdr
 * 0.9.3: exit 0, empty stdout), so the generic `herdr()` wrapper, which wants a JSON envelope, does not fit.
 */
export async function runInPane(pane: string, command: string): Promise<void> {
  assertNoControlChars("pane id", pane);
  assertNoControlChars("pane command", command);
  const session = loadConfig().herdrSession;
  const argv = [...(session === null ? [] : ["--session", session]), "pane", "run", pane, command];
  const r = await run("herdr", argv, { cwd: process.cwd(), env: process.env, timeoutMs: 30_000 });
  if (r.code === 0) return;
  const text = r.stdout.trim() !== "" ? r.stdout : r.stderr;
  let code = "bad_tool_output";
  let message = `herdr pane run exited ${r.code ?? r.signal}`;
  try {
    const e = (JSON.parse(text) as { error?: { code?: unknown; message?: unknown } }).error;
    if (typeof e?.code === "string" && typeof e.message === "string") [code, message] = [`herdr_${e.code}`, e.message];
  } catch {
    // not an error envelope: keep the generic message
  }
  throw failure(code, message, { args: argv, code: r.code, stderr: r.stderr.slice(-2000) });
}

/**
 * Lock the plan, create the Lead Branch worktree, record it as `leadWorktree`, and (unless `watch` is false) start
 * `cpd watch --wf <wf>` in a pane split from the Lead pane. Refusals (`plan_not_approved`, …) happen before anything is written.
 *
 * Re-runnable after a failure past the lock: a Workflow already in phase `dispatch` without a `leadWorktree` continues
 * from the Lead Branch (`createLeadBranch` is idempotent), since a locked plan cannot be locked again.
 */
export async function planLock(wf: string, opts: { watch: boolean }): Promise<PlanLocked> {
  const before = readWorkflow(wf);
  if (opts.watch && before.lead === null) {
    throw refused("lead_not_attached", `Workflow ${wf} has no attached Lead to split the watch pane from; run cpd lead attach, or pass --no-watch`, { wf });
  }
  const resuming = before.leadWorktree === null && (await getPhase(wf)) === "dispatch";
  if (!resuming) await lockPlan(wf);

  const { path } = await createLeadBranch(before.repo, before.leadBranch, before.base);
  const w = readWorkflow(wf); // `lead attach` may have run since
  await writeWorkflow(wf, { ...w, leadWorktree: path });
  logEvent(wf, "info", "lead_worktree", { path, branch: before.leadBranch, resumed: resuming });

  let watchPane: string | null = null;
  if (opts.watch) {
    if (w.lead === null) throw failure("lead_detached", `Workflow ${wf} lost its Lead while locking`, { wf });
    try {
      watchPane = await splitPane(w.lead.paneId, { direction: "right", cwd: path, noFocus: true });
      await runInPane(watchPane, `cpd watch --wf ${wf}`);
    } catch (e) {
      if (!(e instanceof CpdError)) throw e;
      throw failure(
        "watch_pane_failed",
        `the plan is locked and ${before.leadBranch} exists at ${path}, but the watch pane could not be started: ${e.message}. Start it yourself with: cpd watch --wf ${wf}`,
        { wf, leadWorktree: path, watchPane, cause: e.code },
      );
    }
    logEvent(wf, "info", "watch_pane", { pane: watchPane });
  }
  return { leadBranch: before.leadBranch, leadWorktree: path, watchPane };
}
