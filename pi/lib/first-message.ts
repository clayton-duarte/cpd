import { toCpdError, isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { Message } from "./pi.ts";
import { FIRST_MESSAGE_TYPE } from "./updates.ts";

/** §15.7a: true when the session file holds a first message delivered to this Session (the fork-safe rule: a fork's file starts with its source's). */
export function firstMessageDelivered(entries: readonly unknown[], sessionId: string): boolean {
  return entries.some((e) => isObject(e) && e.type === "custom_message" && e.customType === FIRST_MESSAGE_TYPE && isObject(e.details) && e.details.delivered_to === sessionId);
}

/** Registered once, only for a by-user Session whose first message is pending (a Session without one has no such handler, as with the reminder), before the Context updates (pi runs `before_agent_start` handlers in registration order). */
export function registerFirstMessage(host: Host): void {
  if (host.registered.firstMessage) return;
  host.registered.firstMessage = true;
  host.pi.on("before_agent_start", async (_event, ctx) => {
    const r = host.run;
    if (!r || !r.firstPending || (r.info.role !== "lead" && r.info.role !== "manager")) return undefined;
    if (firstMessageDelivered(ctx.sessionManager.getEntries(), r.id)) {
      r.firstPending = false;
      return undefined;
    }
    try {
      const out = await host.call(["session", "first-message", "--session", r.id]);
      if (typeof out.text !== "string" || !Number.isSafeInteger(out.event)) throw new Error("cpd session first-message: the output lacks text or event");
      const event = out.event as number;
      const message: Message = { customType: FIRST_MESSAGE_TYPE, content: out.text, display: false, details: { event, delivered_to: r.id } };
      r.firstPending = false;
      r.firstFailed = false;
      r.firstEvent = Math.max(r.firstEvent, event);
      return { message };
    } catch (e) {
      if (!r.firstFailed) {
        r.firstFailed = true;
        const err = toCpdError(e);
        host.report("first message", err.code, err.message, err.details);
      }
      return undefined;
    }
  });
}
