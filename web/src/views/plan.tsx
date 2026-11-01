// The Plan cockpit view — PR boxes, Job pills, Pulse and the chat slot
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776). The read model carries
// the PR and Job rows as JSON (`docs/commands.md` §4.3); the view renders the fields it
// knows and never invents the open ones.
import type { ReactElement } from "react";
import type { PlanShowResult } from "../api/readmodel";
import { Badge } from "../components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";

/** The Plan view reads `plan show` (§3, §6). */
export interface PlanProps {
  plan: PlanShowResult;
}

/** Reads a string field off a JSON row, or `undefined` when it is absent or not a string. */
function str(row: unknown, key: string): string | undefined {
  if (typeof row !== "object" || row === null) return undefined;
  const value = (row as Record<string, unknown>)[key];
  return typeof value === "string" ? value : undefined;
}

/** Renders the cockpit Plan view (read-only, A7). */
export function Plan({ plan }: PlanProps): ReactElement {
  const { prs, jobs } = plan;
  return (
    <div className="flex flex-col gap-4" data-view="plan">
      <Card>
        <CardHeader>
          <CardTitle>{plan.plan.title}</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-sm">
          <div className="flex items-center gap-2">
            <Badge variant="outline">{plan.plan.phase}</Badge>
            <span className="text-muted-foreground">
              {plan.plan.lead_session ? `lead ${plan.plan.lead_session}` : "no lead"}
            </span>
          </div>
          <div className="text-muted-foreground">
            {prs.length} pull requests · {jobs.length} jobs
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Pull requests</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {prs.length === 0 ? (
            <p className="text-sm text-muted-foreground">No pull requests.</p>
          ) : (
            prs.map((pr, index) => (
              <div key={str(pr, "key") ?? index} className="rounded-md border p-2 text-sm">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{str(pr, "key") ?? "pr"}</span>
                  {str(pr, "repo") ? (
                    <span className="text-muted-foreground">{str(pr, "repo")}</span>
                  ) : null}
                  {str(pr, "status") ? <Badge variant="secondary">{str(pr, "status")}</Badge> : null}
                </div>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Jobs</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          {jobs.length === 0 ? (
            <p className="text-sm text-muted-foreground">No jobs.</p>
          ) : (
            jobs.map((job, index) => (
              <Badge key={str(job, "key") ?? index} variant="outline">
                {str(job, "title") ?? str(job, "key") ?? "job"}
                {str(job, "status") ? ` · ${str(job, "status")}` : ""}
              </Badge>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  );
}
