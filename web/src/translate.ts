// The host-frame translation of the dev live adapter (web/README.md "`web/src/translate.ts` — the
// frame translation", hs-d819/chat-host). It folds host frames onto the same `TranscriptMessage[]`
// the CLI path produces (the fold of docs/contract.md §6.20, in TS), so a durable Session read over
// the socket renders exactly like one read through `cpd session tail`.
//
// Stub: `applyHostFrame` and `translateFrames` throw `not implemented` until the implementation Job
// fills them in — this is what makes `web/src/translate.test.ts` RED.
import type { TranscriptMessage } from "./transcript.ts";
import type { HostFrame } from "./api/host.ts";

/** The fold state: the transcript so far and the cursor of the last applied frame. */
export interface HostState {
  messages: TranscriptMessage[];
  cursor: { epoch: string; seq: number } | undefined;
}

/** Applies one frame; pure: returns the next state. */
export function applyHostFrame(_state: HostState, _frame: HostFrame): HostState {
  throw new Error("not implemented");
}

/** Applies frames in order; the fold of the frames stream and the translation tests. */
export function translateFrames(_state: HostState, _frames: readonly HostFrame[]): HostState {
  throw new Error("not implemented");
}
