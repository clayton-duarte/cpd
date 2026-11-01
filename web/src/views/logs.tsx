// The Logs cockpit view — the events log, `HH:MM:SS` + text/kind
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776).
import type { ReactElement } from "react";
import { Badge } from "../components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

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

/** The `HH:MM:SS` of an ISO timestamp, taken verbatim from the record (never re-zoned). */
function clock(at: string): string {
  const match = /T(\d{2}:\d{2}:\d{2})/.exec(at);
  return match?.[1] ?? at;
}

/** Renders the cockpit Logs view (read-only, A7). */
export function Logs({ events }: LogsProps): ReactElement {
  return (
    <Card data-view="logs">
      <CardHeader>
        <CardTitle>Logs</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-1 font-mono text-xs">
        {events.length === 0 ? (
          <p className="text-muted-foreground">No events.</p>
        ) : (
          events.map((event) => (
            <div key={event.id} className="flex items-baseline gap-2">
              <span className="text-muted-foreground">{clock(event.at)}</span>
              <Badge variant="secondary">{event.kind}</Badge>
              <span>{event.text}</span>
            </div>
          ))
        )}
      </CardContent>
    </Card>
  );
}
