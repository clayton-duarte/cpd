import { sessionMessage } from "../lib/delivery.ts";
import type { CpdEvent } from "../lib/event.ts";

export type Input = { content: string; requestId: string; whenBusy: "followUp" | "steer" };

export function inputOf(event: CpdEvent, session: string): Input | undefined {
  if (event.session !== session) return undefined;
  const input = (content: string, whenBusy: Input["whenBusy"] = "followUp"): Input => ({ content, requestId: `event:${event.id}`, whenBusy });
  if (event.kind === "attempt-nudge") return input(event.data.prompt);
  if (event.kind === "job-addendum") return input(event.text, "steer");
  if (event.kind === "session-message") {
    const from = event.data.from;
    return input(sessionMessage(event, session, from === null ? null : { id: from, name: null }).content);
  }
  return undefined;
}
