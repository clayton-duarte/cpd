// The Inbox cockpit view — the kinds with per-second age (docs/research/web-client-api.md
// §6, read-only per A7; hs-d776). The kinds include `deploy`; an item with a `url` renders
// as a link; the synthetic `engine-down` row (id:null, `core/src/inbox/cmd.rs:150-168`)
// renders from `engine.running`.
import type { ReactElement } from "react";
import type { InboxListResult } from "../api/readmodel";
import { Badge } from "../components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

/** The Inbox view reads `inbox list` (§3, §6). */
export interface InboxProps {
  inbox: InboxListResult;
}

/** Formats an age in seconds as `Ns` / `Nm` / `Nh`. */
function formatAge(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}

/** Renders the cockpit Inbox (read-only, A7). */
export function Inbox({ inbox }: InboxProps): ReactElement {
  return (
    <div className="flex flex-col gap-4" data-view="inbox">
      {inbox.engine.running ? null : (
        <Card className="border-destructive">
          <CardContent className="text-sm text-destructive">
            The engine is down. Items below are read-only until it returns.
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Inbox</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {inbox.items.length === 0 ? (
            <p className="text-sm text-muted-foreground">No open items.</p>
          ) : (
            inbox.items.map((item) => (
              <div
                key={item.id ?? item.kind}
                className="flex items-center gap-2 rounded-md border p-2 text-sm"
              >
                <Badge variant="outline">{item.label}</Badge>
                <span className="font-medium">{item.kind}</span>
                <span>{item.subject}</span>
                {item.url ? (
                  <a className="text-primary underline-offset-4 hover:underline" href={item.url}>
                    open
                  </a>
                ) : null}
                <span className="ml-auto text-muted-foreground">{formatAge(item.age_seconds)}</span>
              </div>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  );
}
