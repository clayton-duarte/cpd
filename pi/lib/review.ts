import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { CpdError, isObject, toCpdError } from "./cpd.ts";
import { FOLLOW_UP, send } from "./delivery.ts";
import type { EventOf } from "./event.ts";
import type { Host } from "./host.ts";
import { DECISION_TYPE, decisionText, ELSEWHERE_CHAT_NOTE, ENTRY_TYPE, reviewNumber, REVIEW_LABEL, SUMMARY_TYPE, type ReviewEntry } from "./plan-review.ts";
import { closeDialog, closeOlderDialogs, runDialog } from "./review-chat.ts";
import type { Run } from "./run.ts";

// The Plan review in chat (§15.5).

export const QUESTION_LABEL = "φ: Worker question";

const runDir = (host: Host, plan: string) => join(host.env.CPD_HOME || join(homedir(), ".cpd"), "run", plan);
const summaryFile = (host: Host, plan: string, review: string) => join(runDir(host, plan), `summary-${review}.md`);
const feedbackFile = (host: Host, plan: string, review: string) => join(runDir(host, plan), `feedback-${review}.md`);

/** §15.5 "Latest only": a pending request for `review` closes the reasons of this session's older reviews. It does not report: the caller refreshes once the request is handled, so the Lead does not flicker to `idle` between the two reviews. */
export function supersede(r: Run, review: string): void {
  for (const known of r.reviews.keys()) if (reviewNumber(known) < reviewNumber(review)) r.facts.close(`plan-review:${known}`);
  closeOlderDialogs(r, review);
}

/** Reads `file` as text; a failure is `file_unreadable`. */
function readText(file: string): string {
  try {
    return readFileSync(file, "utf8");
  } catch (e) {
    throw new CpdError("file_unreadable", `cannot read ${file}: ${e instanceof Error ? e.message : String(e)}`);
  }
}

/** §15.5 "The dialog": the answers of a chat review, run detached by `showDialog`. */
function answersOf(host: Host, r: Run, review: string) {
  return {
    async decide(approved: boolean, feedback: string | undefined) {
      const entry = r.reviews.get(review);
      if (entry) await decide(host, r, entry, approved, feedback);
    },
  };
}

/** Opens the dialog of the pending chat review `review` and returns at once: it runs detached (§15.5 "Opening" step 6), its failures are delivered. */
export function showDialog(host: Host, r: Run, review: string, title: string | undefined): void {
  void runDialog(host, r, review, title, answersOf(host, r, review)).catch((e) => host.deliver(`plan review ${review}`, e));
}

/** Opens a pending review: its summary and dialog in chat. Errors are delivered here; nothing is written to the session on failure. */
export async function openReview(host: Host, r: Run, review: string, via: "event" | "tool", paths: { summary?: string } = {}): Promise<void> {
  const { pi } = host;
  const plan = r.plan;
  const known = r.reviews.get(review);
  if (known && (via === "event" || known.status === "pending")) return;
  if (r.opening.has(review)) return;
  r.opening.add(review);
  try {
    const latest = (await host.call(["plan", "show", "--plan", plan])).review;
    if (!isObject(latest) || latest.id !== review || latest.status === "approved" || latest.status === "rejected" || latest.status === "superseded") return;
    const summaryPath = paths.summary ?? summaryFile(host, plan, review);
    const content = readText(summaryPath);
    if (content.trim() === "") throw new CpdError("summary_empty", `the summary file ${summaryPath} is empty`);
    if (host.run !== r) return;
    pi.sendMessage({ customType: SUMMARY_TYPE, content, display: true, details: { plan, review } }, { triggerTurn: false });
    const entry: ReviewEntry = { plan, review, surface: "chat", status: "pending", summary: content, ...(typeof latest.title === "string" ? { title: latest.title } : {}) };
    r.reviews.set(review, entry);
    pi.appendEntry(ENTRY_TYPE, entry);
    r.facts.open(`plan-review:${review}`, REVIEW_LABEL, "human");
    host.refresh(r);
    await r.reporter.settled(); // the blocked state is reported before the dialog takes the editor
    if (host.run === r) showDialog(host, r, review, typeof latest.title === "string" ? latest.title : undefined);
  } catch (e) {
    host.deliver(`plan review ${review}`, e);
  } finally {
    r.opening.delete(review);
  }
}

const tell = (host: Host, r: Run, content: string, details: Record<string, unknown>) =>
  send(host, r, { customType: DECISION_TYPE, content, display: true, details }, FOLLOW_UP(true));

/** Appends the settled entry, closes the blocking reason and tells the Lead (§15.5 "Deciding" steps 2 and 3). */
function settle(host: Host, r: Run, entry: ReviewEntry, verdict: boolean, text: string | undefined, current: boolean, elsewhere: boolean): void {
  const done: ReviewEntry = { ...entry, status: verdict ? "approved" : "rejected" };
  r.reviews.set(entry.review, done);
  host.pi.appendEntry(ENTRY_TYPE, done);
  if (r.facts.close(`plan-review:${entry.review}`)) host.refresh(r);
  closeDialog(r, entry.review);
  tell(host, r, decisionText(entry.review, verdict, text, current, elsewhere ? ELSEWHERE_CHAT_NOTE : undefined), {
    plan: entry.plan,
    review: entry.review,
    approved: verdict,
    current,
    ...(text !== undefined ? { feedback: text } : {}),
    ...(elsewhere ? { decided_elsewhere: true } : {}),
  });
}

/**
 * §15.5 "Deciding elsewhere": a `plan-review` Event with status `approved` or `rejected` for a pending entry means another caller decided it. cpd already has the decision, so there is no `plan decide`. A failure leaves the entry pending.
 */
export async function decidedElsewhere(host: Host, r: Run, entry: ReviewEntry, ev: EventOf<"plan-review">): Promise<void> {
  if (entry.status !== "pending" || r.deciding.has(entry.review)) return;
  r.deciding.add(entry.review);
  try {
    const verdict = ev.data.status === "approved";
    const review = (await host.call(["plan", "show", "--plan", r.plan])).review;
    const shown = isObject(review) && review.id === entry.review ? review : undefined;
    const current = shown !== undefined && shown.current !== false;
    const feedback = typeof ev.data.feedback === "string" ? ev.data.feedback : shown !== undefined && typeof shown.feedback === "string" ? shown.feedback : undefined;
    if (host.run !== r || r.reviews.get(entry.review)?.status !== "pending") return;
    settle(host, r, entry, verdict, feedback, current, true);
  } finally {
    r.deciding.delete(entry.review);
  }
}

/** §15.5 "Latest only": a `plan-review` Event with status `superseded` closes the reason and the dialog of a pending entry and records it. Nothing is sent. */
export function closeSuperseded(host: Host, r: Run, entry: ReviewEntry): void {
  if (r.reviews.get(entry.review)?.status !== "pending") return;
  const done: ReviewEntry = { ...entry, status: "superseded" };
  r.reviews.set(entry.review, done);
  host.pi.appendEntry(ENTRY_TYPE, done);
  if (r.facts.close(`plan-review:${entry.review}`)) host.refresh(r);
  closeDialog(r, entry.review);
}

/** Records the User's decision with cpd, then tells the Lead. A failure other than `review_decided` leaves the entry pending. */
export async function decide(host: Host, r: Run, entry: ReviewEntry, approved: boolean, feedback: string | undefined): Promise<void> {
  if (entry.status !== "pending" || r.deciding.has(entry.review)) return;
  r.deciding.add(entry.review);
  let verdict = approved;
  let text = feedback;
  let current = true;
  try {
    const plan = r.plan;
    const args = ["plan", "decide", entry.review, approved ? "--approve" : "--reject"];
    if (feedback !== undefined && feedback !== "") {
      const file = feedbackFile(host, plan, entry.review);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, feedback);
      args.push("--feedback-file", file);
    }
    args.push("--plan", plan);
    try {
      const out = await host.call(args);
      current = out.current !== false;
    } catch (e) {
      if (toCpdError(e).code !== "review_decided") throw e;
      // Another caller decided it: continue with that decision.
      const review = (await host.call(["plan", "show", "--plan", plan])).review;
      if (isObject(review) && review.id === entry.review && (review.status === "approved" || review.status === "rejected")) {
        verdict = review.status === "approved";
        text = typeof review.feedback === "string" ? review.feedback : undefined;
        current = review.current !== false;
      } else {
        current = false; // a newer review exists: this decision no longer counts
      }
    }
    if (host.run !== r) return;
    settle(host, r, entry, verdict, text, current, false);
  } catch (e) {
    host.deliver(`plan decide ${entry.review}`, e);
  } finally {
    r.deciding.delete(entry.review);
  }
}

export function registerTool(host: Host): void {
  if (host.registered.tool) return;
  host.registered.tool = true;
  host.pi.registerTool({
    name: "cpd_plan_review",
    label: "cpd plan review",
    description:
      "Ask the User to review and approve the current cpd Plan (its draft Jobs): a short summary in the chat and one dialog (Approve, Change). Returns at once; the decision arrives later as a cpd message. Call it once per revision.",
    parameters: {
      type: "object",
      properties: { title: { type: "string", description: "One line of at most 72 characters naming what the drafts under review do" } },
      required: ["title"],
      additionalProperties: false,
    },
    exposure: "model-only",
    async execute(_id: string, params: { title?: unknown }) {
      const r = host.run;
      if (!r || !r.info.planLead) throw new CpdError("not_a_lead", "cpd_plan_review is only available in a Plan Lead's session");
      const out = await host.call(["plan", "review", "--plan", host.env.CPD_PLAN || r.plan, "--title", typeof params?.title === "string" ? params.title : ""]);
      const review = isObject(out.review) && typeof out.review.id === "string" ? out.review.id : undefined;
      const already = out.already === true;
      if (review !== undefined && already) {
        const known = r.reviews.get(review);
        if (known?.status !== "pending") await openReview(host, r, review, "tool", { summary: typeof out.summary === "string" ? out.summary : undefined });
      }
      const next = typeof out.next === "string" ? out.next : "The User reviews the plan summary in the chat. Stop and wait for the decision message.";
      return { content: [{ type: "text" as const, text: next }], details: { review, already } };
    },
  });
}
