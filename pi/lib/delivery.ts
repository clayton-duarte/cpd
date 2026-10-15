import type { CpdEvent, Kind } from "./event.ts";
import type { Message, SendOptions } from "./pi.ts";

/** commands.md §15.3, "Plan Lead" column: the kinds that become a message, and whether they trigger a turn. */
export const LEAD_MESSAGES: Partial<Record<Kind, { turn: boolean }>> = {
  "job-merged": { turn: true },
  "job-failed": { turn: true },
  "all-merged": { turn: true },
  "pr-feedback": { turn: true },
  "plan-done": { turn: true },
  error: { turn: true },
  escalation: { turn: true },
  "attempt-retry": { turn: false },
  "job-cancelled": { turn: false },
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
