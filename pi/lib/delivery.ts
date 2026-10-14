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
  rename: { deliver: false, triggerTurn: false },
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

/** An effect on the session itself, not a message to the Lead (contract §10, `rename`). Never triggers a turn. */
export type Action = { type: "set-name"; name: string; notice: string };

/** The action a valid queue message asks for, or null when its kind has none. */
export function actionOf(msg: LeadMessage): Action | null {
  if (msg.kind !== "rename") return null;
  const name = msg.data.name;
  if (typeof name !== "string") throw new Error("rename message without data.name (validated at the queue boundary)");
  return { type: "set-name", name, notice: `cpd: session renamed to "${name}"` };
}

/** An `error` delivery for something the adapter itself found wrong (fail loud). */
export function errorDelivery(text: string, details: Record<string, unknown> = {}): Delivery {
  return {
    message: { customType: "cpd", content: text, display: true, details: { kind: "error", text, ...details } },
    options: { triggerTurn: DELIVERY.error.triggerTurn, deliverAs: "followUp" },
  };
}
