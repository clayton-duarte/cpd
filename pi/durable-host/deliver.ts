import { leadRule, sessionMessage } from "../lib/delivery.ts";
import type { CpdEvent, EventOf } from "../lib/event.ts";

/** One submission of the host (contract §6.13 "Delivery (P1.4)"). `entry.data.delivered_to` is what the cursor counts. */
export type Input = {
  content: string;
  requestId: string;
  whenBusy: "followUp" | "steer";
  entry?: { kind: string; data: Record<string, unknown> };
};

/**
 * The input of an Event for this Session, or `undefined` (a no-op kind, another Session's Event, a kind the role does not take).
 * `sender` is the name of a `session-message`'s sender, read by the caller; `null` or absent: the id stands in.
 */
export function inputOf(event: CpdEvent, session: string, info: { role: string; planLead: boolean }, sender?: string | null): Input | undefined {
  if (event.session !== session) return undefined;
  const input = (content: string, whenBusy: Input["whenBusy"]): Input => ({
    content,
    requestId: `event:${event.id}`,
    whenBusy,
    entry: { kind: "cpd-event", data: { id: event.id, kind: event.kind, delivered_to: session } },
  });
  if (event.kind === "session-message") {
    const e = event as EventOf<"session-message">;
    const from = e.data.from;
    return input(sessionMessage(e, session, from === null ? null : { id: from, name: sender ?? null }).content, "followUp");
  }
  if (!info.planLead) return undefined;
  const rule = leadRule(event);
  return rule === undefined ? undefined : input(event.text, rule.deliverAs === "steer" ? "steer" : "followUp");
}
