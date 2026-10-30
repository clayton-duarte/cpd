import { isObject } from "./cpd.ts";

/**
 * commands.md §15.2 step 4: the greater of `registered_event` and the highest `details.id` of the `cpd` custom messages
 * in the session's entries (delivery is not in id order: `pr-feedback` and `escalation` are steered, the rest are follow-ups) whose `details.delivered_to` is this Session. A fork's file starts with its source's
 * messages; their `delivered_to` is the source's id, so they never count.
 */
export function cursorFrom(registeredEvent: number, entries: readonly unknown[], sessionId: string): number {
  let delivered = 0;
  for (const e of entries) {
    if (!isObject(e) || e.type !== "custom_message" || e.customType !== "cpd" || !isObject(e.details)) continue;
    if (e.details.delivered_to !== sessionId || !Number.isSafeInteger(e.details.id)) continue;
    delivered = Math.max(delivered, e.details.id as number);
  }
  return Math.max(registeredEvent, delivered);
}
