// The Tree cockpit view — Context/Plan/Session rows, dots, costs and the Inbox strip
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776).
//
// STUB: throws `not implemented` until the views-impl Job fills it in. This is what makes
// the views harness RED (web/src/views/views.test.tsx).
import type { ReactElement } from "react";
import type { InboxListResult, PlanListResult, SessionListResult } from "../api/readmodel";

/** Everything the Tree view reads: Plans, Sessions and the Inbox strip (§3, §6). */
export interface TreeProps {
  plans: PlanListResult;
  sessions: SessionListResult;
  inbox: InboxListResult;
}

/** Renders the cockpit Tree (STUB). */
export function Tree(_props: TreeProps): ReactElement {
  throw new Error("not implemented");
}
