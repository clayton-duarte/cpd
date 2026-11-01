// The Inbox cockpit view — the kinds with per-second age (docs/research/web-client-api.md
// §6, read-only per A7; hs-d776).
//
// STUB: throws `not implemented` until the views-impl Job fills it in. This is what makes
// the views harness RED (web/src/views/views.test.tsx).
import type { ReactElement } from "react";
import type { InboxListResult } from "../api/readmodel";

/** The Inbox view reads `inbox list` (§3, §6). */
export interface InboxProps {
  inbox: InboxListResult;
}

/** Renders the cockpit Inbox (STUB). */
export function Inbox(_props: InboxProps): ReactElement {
  throw new Error("not implemented");
}
