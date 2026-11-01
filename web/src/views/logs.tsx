// The Logs cockpit view — the events log, `HH:MM:SS` + text/kind
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776).
//
// STUB: throws `not implemented` until the views-impl Job fills it in. This is what makes
// the views harness RED (web/src/views/views.test.tsx).
import type { ReactElement } from "react";

/** One line of the events log (web-ui.md §2.1 "Event logs and history"). */
export interface LogEntry {
  id: number;
  at: string;
  kind: string;
  text: string;
}

/** The Logs view reads the events log. */
export interface LogsProps {
  events: readonly LogEntry[];
}

/** Renders the cockpit Logs view (STUB). */
export function Logs(_props: LogsProps): ReactElement {
  throw new Error("not implemented");
}
