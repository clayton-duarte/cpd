import { RouteIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "../../lib/utils.ts";
import type { Dialog, DialogResponse } from "../../protocol/domain.ts";
import { Markdown } from "../chat/Markdown.tsx";
import { Badge } from "../components/badge.tsx";
import { Button } from "../components/button.tsx";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemFooter,
  ItemMedia,
  ItemTitle,
} from "../components/item.tsx";
import { Spinner } from "../components/spinner.tsx";
import { Textarea } from "../components/textarea.tsx";
import { glyphClass } from "../dango-status.ts";

export interface PlanReviewExtras {
  readonly subtitle?: string;
  readonly revision?: string;
  readonly previous?: {
    readonly revision: string;
    readonly verdict: string;
    readonly note?: string;
  };
  readonly onDiff?: () => void;
  readonly workflows?: readonly {
    readonly name: string;
    readonly route: string;
    readonly status: string;
    readonly flag?: string;
    readonly summary: string;
    readonly jobs: readonly {
      readonly name: string;
      readonly role: string;
      readonly tier: string;
      readonly overlap?: boolean;
    }[];
  }[];
  readonly overlaps?: readonly string[];
  readonly risks?: readonly string[];
  readonly outOfScope?: readonly string[];
  readonly stats?: string;
}

const LABEL =
  "mb-2 text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";

export function PlanReviewCard(props: {
  dialog: Extract<Dialog, { method: "cpd_plan_review" }>;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  review?: PlanReviewExtras;
}): React.JSX.Element {
  const { dialog, readOnly, onRespond, review } = props;
  const [asking, setAsking] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [submitting, setSubmitting] = useState<
    "approve" | "reject" | "changes" | null
  >(null);
  const [active, setActive] = useState("outline");
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setSubmitting(null);
    setAsking(false);
    setFeedback("");
  }, [dialog.id]);

  useEffect(() => {
    if (asking) area.current?.focus();
  }, [asking]);

  const respond = (outcome: "approve" | "reject") => {
    if (submitting !== null) return;
    setSubmitting(outcome);
    onRespond({ method: "cpd_plan_review", outcome });
  };
  const trimmed = feedback.trim();

  const workflows = review?.workflows ?? [];
  const overlaps = review?.overlaps ?? [];
  const risks = review?.risks ?? [];
  const oos = review?.outOfScope ?? [];
  const jobCount = workflows.reduce((n, w) => n + w.jobs.length, 0);
  const sections: {
    key: string;
    label: string;
    count?: number;
    tone?: boolean;
  }[] = [{ key: "outline", label: "Outline" }];
  if (workflows.length > 0)
    sections.push({
      key: "workflows",
      label: "Workflows",
      count: workflows.length,
    });
  if (jobCount > 0)
    sections.push({ key: "jobs", label: "Jobs", count: jobCount });
  if (overlaps.length > 0)
    sections.push({
      key: "overlaps",
      label: "Overlaps",
      count: overlaps.length,
      tone: true,
    });
  if (risks.length > 0)
    sections.push({ key: "risks", label: "Risks", count: risks.length });
  if (oos.length > 0)
    sections.push({ key: "oos", label: "Out of scope", count: oos.length });
  const hasOthers = sections.length > 1;
  const elementId = (key: string) =>
    `${dialog.id}-${key === "jobs" ? "workflows" : key}`;
  const list = (items: readonly string[]) => (
    <ul className="m-0 flex list-disc flex-col gap-1.5 pl-4 text-key">
      {items.map((t, i) => (
        <li key={i}>{t}</li>
      ))}
    </ul>
  );
  const hasColumns = overlaps.length > 0 || risks.length > 0 || oos.length > 0;
  const hasSummary = dialog.summary.trim() !== "";
  const hasBody = true;

  return (
    <Item
      variant="outline"
      size="sm"
      asChild
      className="mx-4 mb-3 w-auto shrink-0"
    >
      <section
        aria-label="Plan review"
        data-dialog="cpd_plan_review"
        {...(readOnly ? { "data-readonly": "" } : {})}
        onKeyDown={(e) => {
          if (readOnly || e.key !== "Enter" || !(e.metaKey || e.ctrlKey))
            return;
          if (e.target instanceof HTMLTextAreaElement) return;
          e.preventDefault();
          if (!hasSummary) return;
          respond("approve");
        }}
      >
        <ItemMedia variant="icon" data-part="phi">
          <RouteIcon />
        </ItemMedia>
        <ItemContent data-part="header">
          <ItemTitle>
            <span>Plan review ·</span>
            <h3 className="m-0 font-medium">{dialog.title}</h3>
          </ItemTitle>
          <ItemDescription>
            {review?.subtitle !== undefined ? (
              <span data-part="subtitle" className="font-mono">
                {review.subtitle}
              </span>
            ) : (
              "Review the plan summary and approve or request changes."
            )}
            {review?.previous === undefined ? null : (
              <span data-part="previous" className="ml-2 font-mono">
                {review.previous.revision} · {review.previous.verdict}
                {review.previous.note === undefined
                  ? ""
                  : ` · “${review.previous.note}”`}
              </span>
            )}
            {review?.stats === undefined ? null : (
              <span data-part="stats" className="ml-2 font-mono">
                {review.stats}
              </span>
            )}
          </ItemDescription>
        </ItemContent>
        {readOnly ? null : (
          <ItemActions data-part="actions">
            {review?.onDiff && review.previous ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                data-action="diff"
                onClick={review.onDiff}
              >
                Diff vs {review.previous.revision}
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              size="sm"
              data-action="reject"
              disabled={submitting !== null}
              onClick={() => respond("reject")}
            >
              {submitting === "reject" ? <Spinner /> : null}
              Reject plan
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              data-action="request-changes"
              aria-expanded={asking}
              disabled={submitting !== null}
              onClick={() => setAsking((v) => !v)}
            >
              Request changes
            </Button>
            <Button
              type="button"
              size="sm"
              data-action="approve"
              disabled={submitting !== null || !hasSummary}
              onClick={() => respond("approve")}
            >
              {submitting === "approve" ? <Spinner /> : null}
              Approve &amp; lock{" "}
              <kbd className="font-mono text-[11px]">⌘⏎</kbd>
            </Button>
          </ItemActions>
        )}
        {hasBody || (asking && !readOnly) ? (
          <ItemFooter
            data-part="decision-bar"
            className="flex-col items-stretch gap-3 border-t pt-3"
          >
            {hasBody ? (
              <div data-part="body-row" className="flex min-h-0 gap-3">
                {hasOthers ? (
                  <nav
                    aria-label="Plan sections"
                    data-part="outline-nav"
                    className="flex w-[180px] shrink-0 flex-col gap-0.5 border-r pr-2 text-key"
                  >
                    {sections.map((s) => (
                      <a
                        key={s.key}
                        href={`#${elementId(s.key)}`}
                        data-section={s.key}
                        {...(active === s.key
                          ? { "aria-current": "true" as const }
                          : {})}
                        className={cn(
                          "flex justify-between rounded-md px-2.5 py-1.5",
                          active === s.key
                            ? "bg-muted text-foreground"
                            : "text-muted-foreground hover:text-foreground",
                        )}
                        onClick={(e) => {
                          e.preventDefault();
                          document
                            .getElementById(elementId(s.key))
                            ?.scrollIntoView?.({ block: "start" });
                          setActive(s.key);
                        }}
                      >
                        <span>{s.label}</span>
                        {s.count === undefined ? null : (
                          <span
                            className={cn(
                              "font-mono",
                              s.tone && "text-mitarashi-bright",
                            )}
                          >
                            {s.count}
                          </span>
                        )}
                      </a>
                    ))}
                  </nav>
                ) : null}
                <div
                  data-part="body"
                  className="flex max-h-60 min-w-0 flex-1 flex-col gap-4 overflow-y-auto"
                >
                  <div data-part="outline" id={`${dialog.id}-outline`}>
                    {hasOthers ? <h4 className={LABEL}>Outline</h4> : null}
                    <div className="max-w-[720px] text-sm text-foreground">
                      {hasSummary ? (
                        <Markdown text={dialog.summary} />
                      ) : (
                        <p className="m-0 text-muted-foreground">
                          No plan summary was rendered; approval is disabled.
                        </p>
                      )}
                    </div>
                  </div>
                  {workflows.length > 0 ? (
                    <div
                      data-part="workflows"
                      id={`${dialog.id}-workflows`}
                      className="flex flex-col gap-2.5"
                    >
                      <h4 className={LABEL}>Workflows</h4>
                      {workflows.map((w) => (
                        <article
                          key={w.name}
                          data-part="workflow"
                          data-workflow={w.name}
                          className="flex flex-col gap-2.5 rounded-lg border bg-background px-3 py-2.5"
                        >
                          <div className="flex items-center gap-2.5">
                            <span className="font-mono font-semibold text-foreground">
                              {w.name}
                            </span>
                            <span className="font-mono text-xs text-muted-foreground">
                              {w.route}
                            </span>
                            <Badge
                              tone="decision"
                              data-part="status-pill"
                              className="font-mono"
                            >
                              {w.status}
                            </Badge>
                            {w.flag === undefined ? null : (
                              <span className="font-mono text-xs text-status-done-bright">
                                {w.flag}
                              </span>
                            )}
                            <span className="flex-1" />
                            <span className="font-mono text-xs text-muted-foreground">
                              {w.summary}
                            </span>
                          </div>
                          <div
                            data-part="dag"
                            className="flex flex-wrap items-center gap-1.5 font-mono text-xs"
                          >
                            {w.jobs.map((j, k) => (
                              <span key={j.name} className="contents">
                                {k > 0 ? (
                                  <span
                                    aria-hidden="true"
                                    className="text-muted-foreground"
                                  >
                                    →
                                  </span>
                                ) : null}
                                <span
                                  data-job={j.name}
                                  className="flex items-center gap-1.5 rounded-md border border-dashed px-2 py-0.5"
                                >
                                  <span className="text-foreground">
                                    {j.name}
                                  </span>
                                  <span className="text-muted-foreground">
                                    {j.role} · {j.tier}
                                  </span>
                                  {j.overlap === true ? (
                                    <span className="text-mitarashi-bright">
                                      overlap
                                    </span>
                                  ) : null}
                                </span>
                              </span>
                            ))}
                          </div>
                        </article>
                      ))}
                    </div>
                  ) : null}
                  {hasColumns ? (
                    <div
                      data-part="columns"
                      className="grid grid-cols-3 gap-5"
                    >
                      {overlaps.length > 0 ? (
                        <div
                          data-part="overlaps"
                          id={`${dialog.id}-overlaps`}
                          className="flex flex-col gap-2"
                        >
                          <h4 className={LABEL}>Overlaps</h4>
                          {overlaps.map((t, i) => (
                            <div
                              key={i}
                              data-part="overlap"
                              role="note"
                              className="flex gap-2 rounded-lg border border-status-landing/50 bg-status-landing/[0.07] px-3 py-2.5 text-key"
                            >
                              <span
                                data-tone="landing"
                                data-glyph="landing"
                                aria-hidden="true"
                                className={cn(glyphClass.landing, "mt-1.5")}
                              />
                              <span className="sr-only">overlap: </span>
                              {t}
                            </div>
                          ))}
                        </div>
                      ) : null}
                      {risks.length > 0 ? (
                        <div data-part="risks" id={`${dialog.id}-risks`}>
                          <h4 className={LABEL}>Risks</h4>
                          {list(risks)}
                        </div>
                      ) : null}
                      {oos.length > 0 ? (
                        <div data-part="oos" id={`${dialog.id}-oos`}>
                          <h4 className={LABEL}>Out of scope</h4>
                          {list(oos)}
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              </div>
            ) : null}
            {asking && !readOnly ? (
              <div
                data-part="feedback"
                className="flex flex-col gap-2 rounded-lg border bg-background p-2.5"
              >
                <Textarea
                  ref={area}
                  aria-label="Feedback"
                  rows={2}
                  placeholder="Feedback for the Lead — required to request changes"
                  className="min-h-14 resize-none border-0 bg-transparent p-0 shadow-none focus-visible:ring-0"
                  value={feedback}
                  onChange={(e) => setFeedback(e.target.value)}
                />
                <div className="flex justify-end">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    data-action="send-feedback"
                    disabled={trimmed === "" || submitting !== null}
                    onClick={() => {
                      if (trimmed === "" || submitting !== null) return;
                      setSubmitting("changes");
                      onRespond({
                        method: "cpd_plan_review",
                        outcome: "changes",
                        feedback: trimmed,
                      });
                    }}
                  >
                    {submitting === "changes" ? <Spinner /> : null}
                    Send feedback
                  </Button>
                </div>
              </div>
            ) : null}
          </ItemFooter>
        ) : null}
      </section>
    </Item>
  );
}
