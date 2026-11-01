// The Plan cockpit view — PR boxes, Job pills, Pulse and the chat slot
// (docs/research/web-client-api.md §6, read-only per A7; hs-d776). The read model carries
// the PR and Job rows as JSON (`docs/commands.md` §4.3); the view renders the fields it
// knows and never invents the open ones.
//
// The needs/base links (§6 as amended by web-links-docs; hs-n673): every Job draws one
// link per key in its `needs` to that Job's anchor, and every PR one link per key in its
// `needs` to that PR's anchor plus its `base_pr` as a distinct `on <base_pr>` link. The
// anchors are `data-job`/`data-pr`; the links `data-need`/`data-base` (plan.test.tsx).
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

/**
 * Reads an array field off a JSON row as its string keys. The rows are `unknown`, so a
 * value that is not an array is treated as no needs, and a non-string entry is dropped.
 */
function keys(row: unknown, field: string): string[] {
  if (typeof row !== "object" || row === null) return [];
  const value = (row as Record<string, unknown>)[field];
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
}

/** A link to another row's anchor: `#job-<key>` or `#pr-<key>`. */
function AnchorLink({
  kind,
  need,
  base,
  label,
}: {
  kind: "job" | "pr";
  need?: string;
  base?: string;
  label: string;
}): ReactElement {
  const key = base ?? need ?? "";
  return (
    <a
      data-need={need}
      data-base={base}
      href={`#${kind}-${key}`}
      className="text-muted-foreground underline underline-offset-2 hover:text-foreground"
    >
      {label}
    </a>
  );
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
            prs.map((pr, index) => {
              const key = str(pr, "key");
              const base = str(pr, "base_pr");
              return (
                <div
                  key={key ?? index}
                  data-pr={key}
                  className="flex flex-col gap-1 rounded-md border p-2 text-sm"
                >
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{key ?? "pr"}</span>
                    {str(pr, "repo") ? (
                      <span className="text-muted-foreground">{str(pr, "repo")}</span>
                    ) : null}
                    {str(pr, "status") ? (
                      <Badge variant="secondary">{str(pr, "status")}</Badge>
                    ) : null}
                  </div>
                  {(keys(pr, "needs").length > 0 || base) && (
                    <div className="flex flex-wrap items-center gap-2 text-xs">
                      {keys(pr, "needs").map((need, i) => (
                        <AnchorLink key={`${need}-${i}`} kind="pr" need={need} label={need} />
                      ))}
                      {base ? (
                        <AnchorLink kind="pr" base={base} label={`on ${base}`} />
                      ) : null}
                    </div>
                  )}
                </div>
              );
            })
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
            jobs.map((job, index) => {
              const key = str(job, "key");
              return (
                <div
                  key={key ?? index}
                  data-job={key}
                  className="flex items-center gap-2 rounded-md border p-1"
                >
                  <Badge variant="outline">
                    {str(job, "title") ?? key ?? "job"}
                    {str(job, "status") ? ` · ${str(job, "status")}` : ""}
                  </Badge>
                  {keys(job, "needs").map((need, i) => (
                    <AnchorLink key={`${need}-${i}`} kind="job" need={need} label={need} />
                  ))}
                </div>
              );
            })
          )}
        </CardContent>
      </Card>
    </div>
  );
}
