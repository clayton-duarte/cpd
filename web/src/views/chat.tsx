// The Chat cockpit view — the durable conversation, read-only, rendered from the fold
// (docs/research/web-client-api.md §5, §6; hs-d772, hs-d777).
//
// The view folds the host frames it is handed (the four fold rules, §5) and renders the
// transcript. STUB: throws `not implemented` until the views-impl Job fills it in. This is
// what makes the views harness RED (web/src/views/views.test.tsx).
import type { ReactElement } from "react";
import type { EventsFrame, SnapshotFrame } from "../fold/fold";

/** A host frame the Chat view folds: one `events` commit or a whole-view `snapshot` (§5). */
export type ChatFrame = EventsFrame | SnapshotFrame;

/** The Chat view reads the host frames of one conversation. */
export interface ChatProps {
  /** The epoch the frames belong to (a restart is a new epoch, §5.3). */
  epoch: string;
  frames: readonly ChatFrame[];
}

/** Renders the cockpit Chat from the folded frames (STUB). */
export function Chat(_props: ChatProps): ReactElement {
  throw new Error("not implemented");
}
