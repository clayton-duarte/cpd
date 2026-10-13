import { existsSync } from "node:fs";
import { loadConfig } from "../lib/config.ts";
import { notFound } from "../lib/errors.ts";
import { getJobByKey } from "../ledger/jobs.ts";
import { planPath, reviewBriefPath, specPath } from "../lib/paths.ts";
import { renderPlan } from "./plan.ts";
import { splitAttemptId } from "./spec.ts";
import { loadTemplate, renderTemplate, writeTextAtomic } from "./template.ts";

// The Reviewer's brief (contract §12 step 9, §16): `templates/reviewer.md` rendered for one Attempt.

export type ReviewInputs = { diffPath: string; gatePath: string; worktree: string };

/**
 * Render the brief to `reviews/<attempt-id>.md` and return its path. The Worker Spec must already be rendered
 * (the Reviewer judges against it). With `config.reviewMode` `fresh` the brief also points to `<wf>/plan.md`, which is (re)rendered first.
 */
export async function renderReviewBrief(wf: string, attemptId: string, inputs: ReviewInputs): Promise<string> {
  const { jobKey, number } = splitAttemptId(attemptId);
  const job = await getJobByKey(wf, jobKey);
  const spec = specPath(wf, attemptId);
  if (!existsSync(spec)) throw notFound("spec_missing", `the Worker Spec of ${attemptId} has not been rendered: ${spec}`, { attempt: attemptId, path: spec });
  const fresh = loadConfig().reviewMode === "fresh";
  // A fresh Reviewer reads the plan from disk: make sure `<wf>/plan.md` exists and shows the Workflow as it is now.
  if (fresh) await renderPlan(wf);
  const text = renderTemplate(loadTemplate("reviewer.md"), {
    wf,
    attemptId,
    jobKey,
    jobTitle: job.title,
    number,
    specPath: spec,
    diffPath: inputs.diffPath,
    gatePath: inputs.gatePath,
    worktree: inputs.worktree,
    fresh,
    planPath: planPath(wf),
  });
  const path = reviewBriefPath(wf, attemptId);
  writeTextAtomic(path, text);
  return path;
}
