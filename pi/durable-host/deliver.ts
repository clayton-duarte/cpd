import type { CpdEvent } from "../lib/event.ts";

/** One submission of the host (contract §6.13 "Delivery (P1.4)"). `entry.data.delivered_to` is what the cursor counts. */
export type Input = {
  content: string;
  requestId: string;
  whenBusy: "followUp" | "steer";
  entry?: { kind: string; data: Record<string, unknown> };
};

/** The input of an Event for this Session, or `undefined` (a no-op kind, another Session's Event, a kind the role does not take). */
export function inputOf(_event: CpdEvent, _session: string, _info: { role: string; planLead: boolean }): Input | undefined {
  throw new Error("not implemented");
}
