import { sessionMessage } from "../lib/delivery.ts";
import type { CpdEvent } from "../lib/event.ts";

export type Input = { content: string; requestId: string; whenBusy: "followUp" };

export function inputOf(event: CpdEvent, session: string): Input | undefined {
  if (event.session !== session) return undefined;
  const input = (content: string): Input => ({ content, requestId: `event:${event.id}`, whenBusy: "followUp" });
  if (event.kind === "attempt-nudge") return input(event.data.prompt);
  if (event.kind === "session-message") {
    const from = event.data.from;
    return input(sessionMessage(event, session, from === null ? null : { id: from, name: null }).content);
  }
  return undefined;
}
