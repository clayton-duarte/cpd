import { isObject } from "./cpd.ts";

/**
 * A run is a reply run when it answered a message the User wrote to this Session. Only then does the run's end
 * report `session state idle --reply`.
 */

/**
 * `before_agent_start`: whether the run that starts was started by the User: a prompt typed in pi (`messageType: "user"`),
 * or the adapter's own delivery of a `session-message` nobody sent (`data.from` null; its `details` are the whole Event plus `delivered_to`, §15.3).
 */
export function startsReply(event: unknown): boolean {
  if (!isObject(event)) return false;
  if (event.messageType === "user") return true;
  if (event.messageType !== "custom" || !isObject(event.message)) return false;
  const d = event.message.details;
  return event.message.customType === "cpd" && isObject(d) && typeof d.delivered_to === "string" && d.kind === "session-message" && isObject(d.data) && d.data.from === null;
}

/** `input`: the User typed in pi (also during a run, as a steer or follow-up). */
export const isTyped = (event: unknown): boolean => isObject(event) && event.source === "interactive";
