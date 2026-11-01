// The Plan cockpit view — PR boxes, Job pills, Pulse and the chat slot
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776).
//
// STUB: throws `not implemented` until the views-impl Job fills it in. This is what makes
// the views harness RED (web/src/views/views.test.tsx).
import type { ReactElement } from "react";
import type { PlanShowResult } from "../api/readmodel";

/** The Plan view reads `plan show` (§3, §6). */
export interface PlanProps {
  plan: PlanShowResult;
}

/** Renders the cockpit Plan view (STUB). */
export function Plan(_props: PlanProps): ReactElement {
  throw new Error("not implemented");
}
