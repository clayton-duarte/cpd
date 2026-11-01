import type { CpdEvent, EventOf, Kind } from "./event.ts";
import type { Message, SendOptions } from "./pi.ts";
import { zoneTime } from "./time.ts";

/**
 * commands.md §15.3, "Plan Lead" column: the kinds that become a message, whether they trigger a turn, and how a busy Lead gets them (steer or follow-up).
 * `steer`: pi injects the message after the current tool call. `followUp`: pi delivers it after the run, one per finished run.
 * `pr-feedback` (every reason) and a Worker's `escalation` are urgent, so steered; every other kind that is a message waits as a follow-up; `job-merged` is one only when `data.turn` is true, and the rest (`attempt-retry`, `job-cancelled`, `workflow-started`, `workflow-base-changed`, `pr-merged`) add nothing. `all-merged` is no longer appended (the Workflow's open Job opens the pull request, contract §3.7 E4) and an old one is ignored.
 */
export const LEAD_MESSAGES: Partial<Record<Kind, { turn: boolean; deliverAs: SendOptions["deliverAs"] }>> = {
  "job-merged": { turn: true, deliverAs: "followUp" },
  "job-failed": { turn: true, deliverAs: "followUp" },
  "pr-feedback": { turn: true, deliverAs: "steer" },
  "plan-done": { turn: true, deliverAs: "followUp" },
  error: { turn: true, deliverAs: "followUp" },
  escalation: { turn: true, deliverAs: "steer" },
};

/** The rule of an Event for a Plan Lead; `undefined`: no message (every other kind, and a `job-merged` whose `data.turn` is not true). */
export const leadRule = (ev: CpdEvent) => (ev.kind === "job-merged" && ev.data.turn !== true ? undefined : LEAD_MESSAGES[ev.kind]);

/** §15.7a: while a by-user Session's first message is pending, and §15.15 L4: while the loop guard is tripped, a send that would start a turn is queued for the next run instead. */
export const gated = (r: { firstPending: boolean; tripped: boolean } | undefined, o: SendOptions): SendOptions => ((r?.firstPending || r?.tripped) && o.triggerTurn ? { triggerTurn: false, deliverAs: "nextTurn" } : o);

export const FOLLOW_UP = (turn: boolean): SendOptions => ({ triggerTurn: turn, deliverAs: "followUp" });
export const STEER = (turn: boolean): SendOptions => ({ triggerTurn: turn, deliverAs: "steer" });

/** §15.3: `{customType: "cpd", content: event.text, details: {...event, delivered_to}}`. `delivered_to` is what the cursor counts. */
export function eventMessage(event: CpdEvent, sessionId: string, content: string = event.text): Message {
  return { customType: "cpd", content, display: true, details: { ...event, delivered_to: sessionId } };
}

/** §15.1, "Errors": the adapter's own error, delivered like any message (no `id`, so the cursor never counts it). */
export function errorMessage(what: string, code: string, message: string, extra: Record<string, unknown> = {}): { message: Message; options: SendOptions } {
  return {
    message: { customType: "cpd", content: `φ adapter: ${what}: ${code}: ${message}`, display: true, details: { kind: "error", code, ...extra } },
    options: FOLLOW_UP(false),
  };
}

/** §15.3 `session-message`: `Message from <name> (<id>) (HH:MM ZZZ): <text>`, or `Message from the User (<HH:MM>): <text>` when nobody sent it. The time is the sender's machine-local `at` (pi/lib/time.ts). */
export function sessionMessage(event: EventOf<"session-message">, sessionId: string, sender: { id: string; name: string | null } | null): Message {
  const from = sender === null ? "the User" : sender.name === null ? sender.id : `${sender.name} (${sender.id})`;
  return eventMessage(event, sessionId, `Message from ${from} (${zoneTime(event.at)}): ${event.data.text}`);
}
