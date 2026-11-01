// The Tree cockpit view — Context/Plan/Session rows, dots, costs and the Inbox strip
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776). Every row renders from
// the read-model mirror; the Inbox strip shows the hidden counts and the open items, never
// a mutation surface.
import type { ReactElement } from "react";
import type { InboxListResult, PlanListResult, SessionListResult } from "../api/readmodel";
import { Badge } from "../components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

/** Everything the Tree view reads: Plans, Sessions and the Inbox strip (§3, §6). */
export interface TreeProps {
  plans: PlanListResult;
  sessions: SessionListResult;
  inbox: InboxListResult;
}

/** The one-line state dot colour for a Session's state (§10.1 `state`/`state_label`). */
function stateDot(state: string): string {
  switch (state) {
    case "working":
      return "bg-emerald-500";
    case "waiting":
      return "bg-violet-500";
    case "blocked":
      return "bg-red-500";
    case "idle":
      return "bg-muted-foreground";
    default:
      return "bg-amber-500";
  }
}

/** Renders the cockpit Tree (read-only, A7). */
export function Tree({ plans, sessions, inbox }: TreeProps): ReactElement {
  const contexts = [...new Set(plans.plans.map((plan) => plan.context))];
  return (
    <div className="flex flex-col gap-4" data-view="tree">
      <Card>
        <CardHeader>
          <CardTitle>Contexts</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {contexts.length === 0 ? (
            <p className="text-sm text-muted-foreground">No contexts.</p>
          ) : (
            contexts.map((context) => (
              <div key={context} className="flex items-center gap-2 text-sm">
                <span className="font-medium">{context}</span>
                <Badge variant="secondary">
                  {plans.plans.filter((plan) => plan.context === context).length} plans
                </Badge>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Plans</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {plans.plans.length === 0 ? (
            <p className="text-sm text-muted-foreground">No plans.</p>
          ) : (
            plans.plans.map((plan) => (
              <div key={plan.id} className="flex items-center gap-2 text-sm">
                <span className="font-medium">{plan.title}</span>
                <Badge variant="outline">{plan.phase}</Badge>
                <span className="text-muted-foreground">
                  running {plan.counts.running} · queued {plan.counts.queued} · failed{" "}
                  {plan.counts.failed}
                </span>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Sessions</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {sessions.sessions.length === 0 ? (
            <p className="text-sm text-muted-foreground">No sessions.</p>
          ) : (
            sessions.sessions.map((session) => (
              <div key={session.id} className="flex items-center gap-2 text-sm">
                <span className={`size-2 rounded-full ${stateDot(session.state)}`} />
                <span className="font-medium">{session.title ?? session.name ?? session.id}</span>
                <Badge variant="secondary">{session.state_label}</Badge>
                {session.runner ? <span className="text-muted-foreground">{session.runner}</span> : null}
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Inbox</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-sm">
          <div className="flex items-center gap-2 text-muted-foreground">
            <span>snoozed {inbox.hidden.snoozed}</span>
            <span>dismissed {inbox.hidden.dismissed}</span>
            <span>claimed {inbox.hidden.claimed}</span>
          </div>
          {inbox.items.length === 0 ? (
            <p className="text-muted-foreground">No open items.</p>
          ) : (
            inbox.items.map((item) => (
              <div key={item.id ?? item.kind} className="flex items-center gap-2">
                <Badge variant="outline">{item.label}</Badge>
                <span>{item.subject}</span>
              </div>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  );
}
