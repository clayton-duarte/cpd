import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { CpdError, isObject, toCpdError } from "./cpd.ts";
import { FOLLOW_UP } from "./delivery.ts";
import type { EventOf } from "./event.ts";
import type { Host } from "./host.ts";
import { DECISION_TYPE, decisionText, ENTRY_TYPE, lostText, reviewNumber, REVIEW_LABEL, type ReviewEntry } from "./plan-review.ts";
import { parseOpened, parseStatus, request } from "./plannotator.ts";
import type { Run } from "./run.ts";

// The Plan review through plannotator (§15.5).

export const QUESTION_LABEL = "cpd: Worker question";

const runDir = (host: Host, plan: string) => join(host.env.CPD_HOME || join(homedir(), ".cpd"), "run", plan);
const planFile = (host: Host, plan: string, review: string) => join(runDir(host, plan), `plan-${review}.md`);
const feedbackFile = (host: Host, plan: string, review: string) => join(runDir(host, plan), `feedback-${review}.md`);

/** §15.5 "Latest only": a pending request for `review` closes the reasons of this session's older reviews. It does not report: the caller refreshes once the request is handled, so the Lead does not flicker to `idle` between the two reviews. */
export function supersede(r: Run, review: string): void {
  for (const known of r.reviews.keys()) if (reviewNumber(known) < reviewNumber(review)) r.facts.close(`plan-review:${known}`);
}

/** Opens a pending review in plannotator. Errors are delivered here; nothing is written to the session on failure. */
export async function openReview(host: Host, r: Run, review: string, via: "event" | "tool", path?: string): Promise<void> {
  const { pi } = host;
  const plan = r.plan;
  const known = r.reviews.get(review);
  if (known && (via === "event" || known.status === "pending")) return;
  if (r.opening.has(review)) return;
  r.opening.add(review);
  try {
    const latest = (await host.call(["plan", "show", "--plan", plan])).review;
    if (!isObject(latest) || latest.id !== review || latest.status === "approved" || latest.status === "rejected") return;
    const planFilePath = path ?? planFile(host, plan, review);
    let planContent: string;
    try {
      planContent = readFileSync(planFilePath, "utf8");
    } catch (e) {
      throw new CpdError("file_unreadable", `cannot read ${planFilePath}: ${e instanceof Error ? e.message : String(e)}`);
    }
    const plannotatorId = parseOpened(await request(pi, "plan-review", { planContent, planFilePath }, host.timing.requestTimeoutMs));
    if (host.run !== r) return;
    const entry: ReviewEntry = { plan, review, plannotator_id: plannotatorId, status: "pending" };
    r.reviews.set(review, entry);
    pi.appendEntry(ENTRY_TYPE, entry);
    r.facts.open(`plan-review:${review}`, REVIEW_LABEL);
    host.refresh(r);
  } catch (e) {
    host.deliver(`plan review ${review}`, e);
  } finally {
    r.opening.delete(review);
  }
}

const tell = (host: Host, content: string, details: Record<string, unknown>) =>
  host.pi.sendMessage({ customType: DECISION_TYPE, content, display: true, details }, FOLLOW_UP(true));

/** Appends the settled entry, closes the blocking reason and tells the Lead (§15.5 "Deciding" steps 2 and 3). */
function settle(host: Host, r: Run, entry: ReviewEntry, verdict: boolean, text: string | undefined, current: boolean, elsewhere: boolean): void {
  const done: ReviewEntry = { ...entry, status: verdict ? "approved" : "rejected" };
  r.reviews.set(entry.review, done);
  host.pi.appendEntry(ENTRY_TYPE, done);
  if (r.facts.close(`plan-review:${entry.review}`)) host.refresh(r);
  tell(host, decisionText(entry.review, verdict, text, current, elsewhere), {
    plan: entry.plan,
    review: entry.review,
    plannotator_id: entry.plannotator_id,
    approved: verdict,
    current,
    ...(text !== undefined ? { feedback: text } : {}),
    ...(elsewhere ? { decided_elsewhere: true } : {}),
  });
}

/**
 * §15.5 "Deciding elsewhere": a `plan-review` Event with status `approved` or `rejected` for a pending entry means a caller other than
 * plannotator decided it. cpd already has the decision, so there is no `plan decide`. A failure leaves the entry pending.
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

/** §15.5 "Recovery": `review-status` every `pollMs` until the review is not pending in plannotator. */
export async function recover(host: Host, r: Run, review: string): Promise<void> {
  let reported = false;
  for (;;) {
    const entry = r.reviews.get(review);
    if (host.run !== r || !entry || entry.status !== "pending") return;
    try {
      const status = parseStatus(await request(host.pi, "review-status", { reviewId: entry.plannotator_id }, host.timing.requestTimeoutMs));
      reported = false;
      if (host.run !== r || r.reviews.get(review)?.status !== "pending") return;
      if (status.status === "completed") return await decide(host, r, entry, status.approved, status.feedback);
      if (status.status === "missing") {
        const lost: ReviewEntry = { ...entry, status: "lost" };
        r.reviews.set(review, lost);
        host.pi.appendEntry(ENTRY_TYPE, lost);
        if (r.facts.close(`plan-review:${review}`)) host.refresh(r);
        tell(host, lostText(review), { plan: entry.plan, review, plannotator_id: entry.plannotator_id, lost: true });
        return;
      }
    } catch (e) {
      if (!reported) host.deliver(`plan review ${review}`, e);
      reported = true;
    }
    try {
      await sleep(host.timing.pollMs, undefined, { signal: r.abort.signal });
    } catch {
      return; // shut down
    }
  }
}

export function registerTool(host: Host): void {
  if (host.registered.tool) return;
  host.registered.tool = true;
  host.pi.registerTool({
    name: "cpd_plan_review",
    label: "cpd plan review",
    description:
      "Ask the User to review and approve the current cpd Plan (its draft Jobs) in plannotator. Returns at once; the decision arrives later as a cpd message. Call it once per revision.",
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
        if (known?.status !== "pending") await openReview(host, r, review, "tool", typeof out.path === "string" ? out.path : undefined);
      }
      const next = typeof out.next === "string" ? out.next : "The User reviews the plan in plannotator. Stop and wait for the decision message.";
      return { content: [{ type: "text" as const, text: next }], details: { review, already } };
    },
  });
}
