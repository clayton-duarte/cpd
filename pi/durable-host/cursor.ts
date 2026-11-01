import { isObject } from "../lib/cpd.ts";

/**
 * The Event cursor from the stored entries (contract §6.13 "Delivery (P1.4)", 2): the greater of `registered_event` and the highest delivered id of this Session.
 * Adapted from pi/lib/cursor.ts: pi-durable's entries are `{kind, data}`, not pi custom messages. `cpd-event` carries `data.id`;
 * `cpd-first-message` and `cpd-context` carry `data.event`. A fork's file starts with its source's entries, whose `delivered_to` is the source's id.
 */
export function cursorFrom(registeredEvent: number, entries: readonly unknown[], session: string): number {
  let highest = registeredEvent;
  for (const e of entries) {
    if (!isObject(e) || !isObject(e.data) || e.data.delivered_to !== session) continue;
    const id = e.kind === "cpd-event" ? e.data.id : e.kind === "cpd-first-message" || e.kind === "cpd-context" ? e.data.event : undefined;
    if (typeof id === "number" && Number.isSafeInteger(id)) highest = Math.max(highest, id);
  }
  return highest;
}
