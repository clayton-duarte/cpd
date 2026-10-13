import { z } from "zod";
import { notFound, refused, usage } from "../lib/errors.ts";
import { assertEscalationId } from "../lib/paths.ts";
import { logEvent } from "./audit.ts";
import { createIssue, listIssues, metadataArgs, readLedger, withLedger, type ReadLedger } from "./bd.ts";
import { findAttempt, listAttempts } from "./attempts.ts";
import { BdComments, type BdIssue } from "./schema.ts";

// Contract §8 (Escalation mapping), §11 (`ask`, `answer`). An Escalation is a child issue of the Attempt,
// open while the Worker waits. `bd close <attempt>` is refused while one is open (t2 F4): the Attempt
// functions surface that as `escalation_open`.

export type Escalation = {
  /** `<attempt-id>-q<k>` */
  id: string;
  /** beads issue id */
  issue: string;
  attempt: string;
  question: string;
  askedAt: string | null;
  status: string;
  answeredBy: string | null;
  answeredAt: string | null;
};

const str = (m: Record<string, unknown> | undefined, k: string): string | null => (typeof m?.[k] === "string" ? (m[k] as string) : null);

/** Escalation id → Attempt id (`<attempt-id>-q<k>`). */
const attemptOfEscalation = (escId: string) => escId.slice(0, escId.lastIndexOf("-q"));

function toEscalation(i: BdIssue, attempt: string): Escalation {
  return {
    id: str(i.metadata, "esc_id") ?? i.id,
    issue: i.id,
    attempt,
    question: i.description ?? "",
    askedAt: str(i.metadata, "asked_at"),
    status: i.status,
    answeredBy: str(i.metadata, "answered_by"),
    answeredAt: str(i.metadata, "answered_at"),
  };
}

/** Every Escalation of the Workflow (any status) as raw issues with their Attempt id. */
async function allEscalations(l: ReadLedger): Promise<{ issue: BdIssue; attempt: string }[]> {
  const attempts = await listAttempts(l.wf);
  const out: { issue: BdIssue; attempt: string }[] = [];
  for (const issue of await listIssues(l, "escalation")) {
    const a = attempts.find((x) => x.issue === issue.parent);
    out.push({ issue, attempt: a?.id ?? attemptOfEscalation(str(issue.metadata, "esc_id") ?? "") });
  }
  return out;
}

const Question = z.strictObject({ escId: z.string(), question: z.string().min(1), askedAt: z.string().min(1) });

/**
 * Record a Worker's question. Idempotent by `escId`: a second call changes nothing and returns
 * `created: false`. The Attempt must be `in_progress` (checked only when the Escalation is new).
 */
export async function recordEscalation(
  wf: string,
  attemptId: string,
  input: { escId: string; question: string; askedAt: string },
): Promise<{ escalation: Escalation; created: boolean }> {
  const q = Question.safeParse(input);
  if (!q.success) throw usage("invalid_escalation", "invalid Escalation", { issues: q.error.issues });
  assertEscalationId(input.escId);
  if (attemptOfEscalation(input.escId) !== attemptId) {
    throw usage("invalid_escalation_id", `Escalation ${input.escId} does not belong to Attempt ${attemptId}`, { escId: input.escId, attempt: attemptId });
  }
  return withLedger(wf, `escalation ${input.escId}`, async (l) => {
    const existing = (await allEscalations(l)).find((e) => str(e.issue.metadata, "esc_id") === input.escId);
    if (existing) return { escalation: toEscalation(existing.issue, existing.attempt), created: false };
    const { attempt, job } = await findAttempt(l, attemptId);
    if (attempt.status !== "in_progress") {
      throw refused("illegal_transition", `cannot record an Escalation: Attempt ${attemptId} is ${attempt.status}`, { attempt: attemptId, status: attempt.status, job: job.key });
    }
    const id = await createIssue(l, [
      `--title=Escalation: ${input.escId}`,
      "-t",
      "escalation",
      "--parent",
      attempt.issue,
      `--description=${input.question}`,
      "--metadata",
      JSON.stringify({ esc_id: input.escId, asker: `worker:${attemptId}`, asked_at: input.askedAt }),
    ]);
    logEvent(wf, "info", "escalation_recorded", { attempt: attemptId, escalation: input.escId });
    const created = (await allEscalations(l)).find((e) => e.issue.id === id)!;
    return { escalation: toEscalation(created.issue, created.attempt), created: true };
  });
}

const Answer = z.strictObject({ answer: z.string().min(1), answeredAt: z.string().min(1) });

/**
 * Record the Lead's answer: a comment, `answered_by` / `answered_at` (write-once) and close with reason
 * `answered`. Idempotent: an Escalation that is already closed returns `recorded: false`; a half-done
 * earlier call is completed without repeating its steps.
 */
export async function recordAnswer(wf: string, escId: string, input: { answer: string; answeredAt: string }): Promise<{ escalation: Escalation; recorded: boolean }> {
  assertEscalationId(escId);
  const a = Answer.safeParse(input);
  if (!a.success) throw usage("invalid_answer", "invalid answer", { issues: a.error.issues });
  return withLedger(wf, `answer ${escId}`, async (l) => {
    const found = (await allEscalations(l)).find((e) => str(e.issue.metadata, "esc_id") === escId);
    if (!found) throw notFound("escalation_not_found", `no Escalation ${escId}`, { escalation: escId });
    const { issue, attempt } = found;
    if (issue.status === "closed") return { escalation: toEscalation(issue, attempt), recorded: false };

    if (str(issue.metadata, "answered_by") === null) {
      await l.write(["update", issue.id, ...metadataArgs(issue.metadata, { answered_by: "lead", answered_at: input.answeredAt }, true)]);
    }
    const comments = await l.read(["comments", issue.id], BdComments);
    if (comments.length === 0) await l.write(["comments", "add", issue.id, input.answer]);
    await l.write(["close", issue.id, "--reason", "answered"]);
    logEvent(wf, "info", "escalation_answered", { attempt, escalation: escId });
    const after = (await allEscalations(l)).find((e) => e.issue.id === issue.id)!;
    return { escalation: toEscalation(after.issue, after.attempt), recorded: true };
  });
}

/** Escalations not yet answered, oldest first. */
export async function openEscalations(wf: string): Promise<Escalation[]> {
  return (await allEscalations(readLedger(wf)))
    .filter((e) => e.issue.status !== "closed")
    .map((e) => toEscalation(e.issue, e.attempt))
    .sort((a, b) => (a.askedAt ?? "").localeCompare(b.askedAt ?? "") || a.id.localeCompare(b.id));
}
