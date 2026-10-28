import type { CpdEvent } from "../lib/event.ts";

export type Input = { content: string; requestId: string; whenBusy: "followUp" };

export function inputOf(_event: CpdEvent, _session: string): Input | undefined {
  throw new Error("not implemented");
}
