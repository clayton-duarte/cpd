import { z } from "zod";
import { listIssues, readLedger } from "../ledger/bd.ts";
import { getJobByKey } from "../ledger/jobs.ts";
import { BdComments } from "../ledger/schema.ts";
import { failure, notFound } from "../lib/errors.ts";
import { assertAttemptId, specPath } from "../lib/paths.ts";
import { loadTemplate, renderTemplate, writeTextAtomic } from "./template.ts";

// The Worker Spec (contract §12 step 3, §16): `templates/worker.md` rendered for one Attempt.

/** `<job-key>-a<n>` → its parts (the id is validated first). */
export function splitAttemptId(attemptId: string): { jobKey: string; number: number } {
  assertAttemptId(attemptId);
  const i = attemptId.lastIndexOf("-a");
  return { jobKey: attemptId.slice(0, i), number: Number(attemptId.slice(i + 2)) };
}

const AttemptMeta = z.looseObject({
  number: z.number().int().positive(),
  tier: z.string().optional(),
  worktree: z.string().optional(),
  outcome: z.string().optional(),
  cause: z.string().optional(),
});

/** An Attempt issue's metadata, parsed once at the edge: malformed metadata is a named `attempt_metadata_invalid`, not a raw ZodError. */
function parseAttemptMeta(issueId: string, metadata: unknown): z.infer<typeof AttemptMeta> {
  const parsed = AttemptMeta.safeParse(metadata);
  if (!parsed.success) throw failure("attempt_metadata_invalid", `Attempt issue ${issueId} has malformed metadata`, { id: issueId, issues: parsed.error.issues });
  return parsed.data;
}

const quote = (text: string) => text.split("\n").map((l) => (l === "" ? ">" : `> ${l}`)).join("\n");

/**
 * Render the Worker Spec of `attemptId` to `specs/<attempt-id>.md` and return its path.
 * Reads the Job and its Attempts from the Ledger. The Attempt must already carry its `tier` and `worktree`
 * (contract §12 step 2); a missing value is a `template_missing_value` error, not a default.
 */
export async function renderWorkerSpec(wf: string, attemptId: string): Promise<string> {
  const { jobKey, number } = splitAttemptId(attemptId);
  const job = await getJobByKey(wf, jobKey);
  const l = readLedger(wf);
  const attempts = (await listIssues(l, "attempt", ["--parent", job.id]))
    .map((issue) => ({ issue, meta: parseAttemptMeta(issue.id, issue.metadata) }))
    .sort((a, b) => a.meta.number - b.meta.number);
  const current = attempts.find((a) => a.meta.number === number);
  if (!current) throw notFound("attempt_not_found", `Job ${jobKey} has no Attempt ${number}`, { attempt: attemptId });

  const previous = await Promise.all(
    attempts
      .filter((a) => a.meta.number < number)
      .map(async (a) => {
        const feedback = (await l.read(["comments", a.issue.id], BdComments)).map((c) => c.text.trim()).filter((t) => t !== "").join("\n\n");
        return {
          number: a.meta.number,
          tier: a.meta.tier,
          outcome: a.meta.outcome,
          cause: a.meta.cause ?? "",
          feedback: feedback === "" ? "" : quote(feedback),
        };
      }),
  );

  const text = renderTemplate(loadTemplate("worker.md"), {
    wf,
    attemptId,
    jobKey,
    jobTitle: job.title,
    number,
    tier: current.meta.tier,
    worktree: current.meta.worktree,
    goal: job.goal,
    context: job.context,
    touches: job.touches,
    acceptance: job.acceptance,
    outOfScope: job.outOfScope,
    noOutOfScope: job.outOfScope.length === 0,
    previous,
  });
  const path = specPath(wf, attemptId);
  writeTextAtomic(path, text);
  return path;
}
