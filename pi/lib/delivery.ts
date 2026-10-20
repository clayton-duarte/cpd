import type { CpdEvent, EventOf, Kind } from "./event.ts";
import type { Message, SendOptions } from "./pi.ts";

/**
 * commands.md §15.3, "Plan Lead" column: the kinds that become a message, whether they trigger a turn, and how a busy Lead gets them (steer or follow-up).
 * `steer`: pi injects the message after the current tool call. `followUp`: pi delivers it after the run, one per finished run.
 * `all-merged` (hs-d334: open the pull request now), `pr-feedback` (every reason) and a Worker's `escalation` are urgent, so steered; every other kind waits as a follow-up.
 */
export const LEAD_MESSAGES: Partial<Record<Kind, { turn: boolean; deliverAs: SendOptions["deliverAs"] }>> = {
  "job-merged": { turn: true, deliverAs: "followUp" },
  "job-failed": { turn: true, deliverAs: "followUp" },
  "all-merged": { turn: true, deliverAs: "steer" },
  "pr-feedback": { turn: true, deliverAs: "steer" },
  "plan-done": { turn: true, deliverAs: "followUp" },
  error: { turn: true, deliverAs: "followUp" },
  escalation: { turn: true, deliverAs: "steer" },
  "attempt-retry": { turn: false, deliverAs: "followUp" },
  "job-cancelled": { turn: false, deliverAs: "followUp" },
  "pr-started": { turn: false, deliverAs: "followUp" },
  "pr-merged": { turn: false, deliverAs: "followUp" },
};

export const FOLLOW_UP = (turn: boolean): SendOptions => ({ triggerTurn: turn, deliverAs: "followUp" });

/** §15.3: `{customType: "cpd", content: event.text, details: {...event, delivered_to}}`. `delivered_to` is what the cursor counts. */
export function eventMessage(event: CpdEvent, sessionId: string, content: string = event.text): Message {
  return { customType: "cpd", content, display: true, details: { ...event, delivered_to: sessionId } };
}

/** §15.1, "Errors": the adapter's own error, delivered like any message (no `id`, so the cursor never counts it). */
export function errorMessage(what: string, code: string, message: string, extra: Record<string, unknown> = {}): { message: Message; options: SendOptions } {
  return {
    message: { customType: "cpd", content: `cpd adapter: ${what}: ${code}: ${message}`, display: true, details: { kind: "error", code, ...extra } },
    options: FOLLOW_UP(true),
  };
}

/** §15.3 `session-message`: `Message from <name> (<id>): <text>`, or `Message from the User: <text>` when nobody sent it. */
export function sessionMessage(event: EventOf<"session-message">, sessionId: string, sender: { id: string; name: string | null } | null): Message {
  const from = sender === null ? "the User" : sender.name === null ? sender.id : `${sender.name} (${sender.id})`;
  return eventMessage(event, sessionId, `Message from ${from}: ${event.data.text}`);
}
