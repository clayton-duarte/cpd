import type { LeadMessage, MessageKind } from "../../src/queue/messages.ts";

/** Contract §10, "Adapter delivery" column. `deliver: false` means no message reaches the Lead. */
export type DeliveryRule = { deliver: boolean; triggerTurn: boolean };

/** A `Record` over every kind, so adding a kind to the schema fails the typecheck here. */
export const DELIVERY: Record<MessageKind, DeliveryRule> = {
  "job-merged": { deliver: true, triggerTurn: true },
  "attempt-retry": { deliver: true, triggerTurn: false },
  "job-failed": { deliver: true, triggerTurn: true },
  escalation: { deliver: true, triggerTurn: true },
  "escalation-answered": { deliver: false, triggerTurn: false },
  "review-needed": { deliver: true, triggerTurn: true },
  "all-merged": { deliver: true, triggerTurn: true },
  "pr-feedback": { deliver: true, triggerTurn: true },
  "workflow-done": { deliver: true, triggerTurn: true },
  error: { deliver: true, triggerTurn: true },
};

/** Arguments of `pi.sendMessage(message, options)` (contract §15). */
export type Delivery = {
  message: { customType: "cpd"; content: string; display: true; details: unknown };
  options: { triggerTurn: boolean; deliverAs: "followUp" };
};

/** The delivery for a valid queue message, or null when its kind produces none. */
export function deliveryOf(msg: LeadMessage): Delivery | null {
  const rule = DELIVERY[msg.kind];
  if (!rule.deliver) return null;
  return {
    message: { customType: "cpd", content: msg.text, display: true, details: msg },
    options: { triggerTurn: rule.triggerTurn, deliverAs: "followUp" },
  };
}

/** An `error` delivery for something the adapter itself found wrong (fail loud). */
export function errorDelivery(text: string, details: Record<string, unknown> = {}): Delivery {
  return {
    message: { customType: "cpd", content: text, display: true, details: { kind: "error", text, ...details } },
    options: { triggerTurn: DELIVERY.error.triggerTurn, deliverAs: "followUp" },
  };
}
