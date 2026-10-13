import type { LeadMessage } from "../../src/queue/messages.ts";
import { escalationIdOf } from "./queue.ts";

export const BLOCKED_LABEL = "cpd: Worker question";

/** Payload of `pi.events.emit("herdr:blocked", …)`. herdr-agent-state counts one `active: true` per `active: false`. */
export type BlockedEvent = { active: true; label: string } | { active: false };

/**
 * Open escalation ids. An event is produced only when an id actually changes state, so a duplicate
 * `escalation` (or a late `escalation-answered`) never unbalances herdr's own counter.
 */
export function createBlockedCounter() {
  const open = new Set<string>();
  return {
    get count() {
      return open.size;
    },
    open(id: string): BlockedEvent | null {
      if (open.has(id)) return null;
      open.add(id);
      return { active: true, label: BLOCKED_LABEL };
    },
    close(id: string): BlockedEvent | null {
      return open.delete(id) ? { active: false } : null;
    },
    /** Start from the Workflow's open questions: one `active: true` per id. */
    seed(ids: readonly string[]): BlockedEvent[] {
      return ids.flatMap((id) => this.open(id) ?? []);
    },
    /** The blocked event a queue message causes, if any. */
    onMessage(msg: LeadMessage): BlockedEvent | null {
      if (msg.kind === "escalation") return this.open(escalationIdOf(msg));
      if (msg.kind === "escalation-answered") return this.close(escalationIdOf(msg));
      return null;
    },
  };
}
export type BlockedCounter = ReturnType<typeof createBlockedCounter>;
