import { useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { glyphClass } from "../dango-status.ts";
import type { Dialog, DialogResponse } from "../../protocol/domain.ts";

export interface PlanReviewExtras {
  readonly subtitle?: string;
  readonly revision?: string;
  readonly previous?: { readonly revision: string; readonly verdict: string; readonly note?: string };
  readonly onDiff?: () => void;
  readonly workflows?: readonly {
    readonly name: string;
    readonly route: string;
    readonly status: string;
    readonly flag?: string;
    readonly summary: string;
    readonly jobs: readonly { readonly name: string; readonly role: string; readonly tier: string; readonly overlap?: boolean }[];
  }[];
  readonly overlaps?: readonly string[];
  readonly risks?: readonly string[];
  readonly outOfScope?: readonly string[];
  readonly stats?: string;
}

const LABEL = "mb-2 text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";

export function PlanReviewCard(props: {
  dialog: Extract<Dialog, { method: "cpd_plan_review" }>;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  review?: PlanReviewExtras;
}): React.JSX.Element {
  const { dialog, readOnly, onRespond, review } = props;
  const [asking, setAsking] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [active, setActive] = useState("outline");
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (asking) area.current?.focus();
  }, [asking]);
  const respond = (outcome: "approve" | "reject") => onRespond({ method: "cpd_plan_review", outcome });
  const trimmed = feedback.trim();

  const workflows = review?.workflows ?? [];
  const overlaps = review?.overlaps ?? [];
  const risks = review?.risks ?? [];
  const oos = review?.outOfScope ?? [];
  const jobCount = workflows.reduce((n, w) => n + w.jobs.length, 0);
  const sections: { key: string; label: string; count?: number; tone?: boolean }[] = [{ key: "outline", label: "Outline" }];
  if (workflows.length > 0) sections.push({ key: "workflows", label: "Workflows", count: workflows.length });
  if (jobCount > 0) sections.push({ key: "jobs", label: "Jobs", count: jobCount });
  if (overlaps.length > 0) sections.push({ key: "overlaps", label: "Overlaps", count: overlaps.length, tone: true });
  if (risks.length > 0) sections.push({ key: "risks", label: "Risks", count: risks.length });
  if (oos.length > 0) sections.push({ key: "oos", label: "Out of scope", count: oos.length });
  const hasOthers = sections.length > 1;
  const elementId = (key: string) => `${dialog.id}-${key === "jobs" ? "workflows" : key}`;
  const list = (items: readonly string[]) => (
    <ul className="m-0 flex list-disc flex-col gap-1.5 pl-4 text-key">
      {items.map((t, i) => (
        <li key={i}>{t}</li>
      ))}
    </ul>
  );
  const hasColumns = overlaps.length > 0 || risks.length > 0 || oos.length > 0;

  return (
    <section
      aria-label="Plan review"
      data-dialog="cpd_plan_review"
      className="mx-4 mb-3 flex max-h-[min(80vh,720px)] min-h-0 flex-col overflow-hidden rounded-xl border border-status-decision bg-card"
      onKeyDown={(e) => {
        if (readOnly || e.key !== "Enter" || !(e.metaKey || e.ctrlKey)) return;
        if (e.target instanceof HTMLTextAreaElement) return;
        e.preventDefault();
        respond("approve");
      }}
    >
      <header
        data-part="header"
        className="flex h-14 shrink-0 items-center gap-3 border-b border-edge bg-status-decision/10 px-4"
      >
        <span data-part="phi" aria-hidden="true" className="font-display text-[20px] font-bold text-status-decision-bright">
          φ
        </span>
        <div className="flex min-w-0 flex-col">
          <div className="flex items-baseline gap-1.5">
            <span className="text-[14px] font-semibold text-fg-strong">Plan review ·</span>
            <h3 className="m-0 text-[14px] font-semibold text-fg-strong">{dialog.title}</h3>
          </div>
          {review?.subtitle === undefined ? null : (
            <span data-part="subtitle" className="truncate font-mono text-[11.5px] text-muted-foreground">
              {review.subtitle}
            </span>
          )}
        </div>
        <span className="flex-1" />
        {review?.previous === undefined ? null : (
          <span data-part="previous" className="flex gap-2 text-[12px] text-muted-foreground">
            <span className="font-mono text-mitarashi-bright">{review.previous.revision}</span>
            <span>
              {review.previous.verdict}
              {review.previous.note === undefined ? "" : ` · “${review.previous.note}”`}
            </span>
          </span>
        )}
        {review?.onDiff && review.previous && !readOnly ? (
          <Button
            type="button"
            variant="outline"
            data-action="diff"
            onClick={review.onDiff}
            className="h-7 rounded-md border-edge px-2.5 text-[12px] text-fg-strong"
          >
            Diff vs {review.previous.revision}
          </Button>
        ) : null}
      </header>
      <div data-part="body-row" className="flex min-h-0 flex-1">
        {hasOthers ? (
          <nav
            aria-label="Plan sections"
            data-part="outline-nav"
            className="flex w-[180px] shrink-0 flex-col gap-0.5 border-r border-border px-2 py-3.5 text-key"
          >
            {sections.map((s) => (
              <a
                key={s.key}
                href={`#${elementId(s.key)}`}
                data-section={s.key}
                {...(active === s.key ? { "aria-current": "true" as const } : {})}
                className={cn(
                  "flex justify-between rounded-md px-2.5 py-1.5",
                  active === s.key
                    ? "bg-surface-pressed text-fg-strong"
                    : "text-muted-foreground hover:text-fg-strong",
                )}
                onClick={(e) => {
                  e.preventDefault();
                  document.getElementById(elementId(s.key))?.scrollIntoView?.({ block: "start" });
                  setActive(s.key);
                }}
              >
                <span>{s.label}</span>
                {s.count === undefined ? null : (
                  <span className={cn("font-mono", s.tone && "text-mitarashi-bright")}>{s.count}</span>
                )}
              </a>
            ))}
          </nav>
        ) : null}
        <div data-part="body" className="flex min-w-0 flex-1 flex-col gap-5 overflow-y-auto px-[22px] py-[18px]">
          <div data-part="outline" id={`${dialog.id}-outline`}>
            {hasOthers ? <h4 className={LABEL}>Outline</h4> : null}
            <div className="typeset typeset-chat max-w-[720px] text-[14px] text-fg-strong">
              <Markdown remarkPlugins={[remarkGfm]}>{dialog.summary}</Markdown>
            </div>
          </div>
          {workflows.length > 0 ? (
            <div data-part="workflows" id={`${dialog.id}-workflows`} className="flex flex-col gap-2.5">
              <h4 className={LABEL}>Workflows</h4>
              {workflows.map((w) => (
                <article
                  key={w.name}
                  data-part="workflow"
                  data-workflow={w.name}
                  className="flex flex-col gap-2.5 rounded-[9px] border border-border bg-background px-3 py-2.5"
                >
                  <div className="flex items-center gap-2.5">
                    <span className="font-mono font-semibold text-fg-strong">{w.name}</span>
                    <span className="font-mono text-[11.5px] text-muted-foreground">{w.route}</span>
                    <span
                      data-part="status-pill"
                      className="rounded-[4px] bg-status-decision/[0.14] px-1.5 py-px font-mono text-[11px] text-status-decision-bright"
                    >
                      {w.status}
                    </span>
                    {w.flag === undefined ? null : (
                      <span className="font-mono text-[11px] text-status-done-bright">{w.flag}</span>
                    )}
                    <span className="flex-1" />
                    <span className="font-mono text-[11.5px] text-muted-foreground">{w.summary}</span>
                  </div>
                  <div data-part="dag" className="flex flex-wrap items-center gap-1.5 font-mono text-[12px]">
                    {w.jobs.map((j, k) => (
                      <span key={j.name} className="contents">
                        {k > 0 ? (
                          <span aria-hidden="true" className="text-line">
                            →
                          </span>
                        ) : null}
                        <span
                          data-job={j.name}
                          className="flex items-center gap-1.5 rounded-[5px] border border-dashed border-status-decision px-2 py-[3px]"
                        >
                          <span className="text-fg-strong">{j.name}</span>
                          <span className="text-muted-foreground">
                            {j.role} · {j.tier}
                          </span>
                          {j.overlap === true ? <span className="text-mitarashi-bright">overlap</span> : null}
                        </span>
                      </span>
                    ))}
                  </div>
                </article>
              ))}
            </div>
          ) : null}
          {hasColumns ? (
            <div data-part="columns" className="grid grid-cols-3 gap-5">
              {overlaps.length > 0 ? (
                <div data-part="overlaps" id={`${dialog.id}-overlaps`} className="flex flex-col gap-2">
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
      {readOnly ? null : (
        <footer
          data-part="decision-bar"
          className="flex shrink-0 flex-col gap-2.5 border-t border-edge bg-row-hover px-4 py-3"
        >
          {asking ? (
            <div
              data-part="feedback"
              className="flex flex-col gap-2 rounded-lg border border-input bg-background px-2.5 py-2"
            >
              <textarea
                ref={area}
                aria-label="Feedback"
                rows={2}
                placeholder="Feedback for the Lead — required to request changes"
                className="w-full resize-none bg-transparent text-[13px] text-fg-strong placeholder:text-fg-dim"
                value={feedback}
                onChange={(e) => setFeedback(e.target.value)}
              />
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="outline"
                  data-action="send-feedback"
                  disabled={trimmed === ""}
                  className="h-8 border-line px-3.5 text-fg-strong"
                  onClick={() => onRespond({ method: "cpd_plan_review", outcome: "changes", feedback: trimmed })}
                >
                  Send feedback
                </Button>
              </div>
            </div>
          ) : null}
          <div data-part="actions" className="flex items-center gap-2">
            <Button
              type="button"
              variant="ghost"
              data-action="reject"
              onClick={() => respond("reject")}
              className="h-[34px] rounded-[7px] px-3 text-[13px] text-status-blocked-bright hover:bg-status-blocked/10"
            >
              Reject plan
            </Button>
            <span className="flex-1" />
            {review?.stats === undefined ? null : (
              <span data-part="stats" className="mr-1.5 font-mono text-meta text-muted-foreground">
                {review.stats}
              </span>
            )}
            <Button
              type="button"
              variant="outline"
              data-action="request-changes"
              aria-expanded={asking}
              onClick={() => setAsking(true)}
              className="h-[34px] rounded-[7px] border-line px-3.5 text-[13px] text-fg-strong"
            >
              Request changes
            </Button>
            <Button
              type="button"
              data-action="approve"
              onClick={() => respond("approve")}
              className="h-[34px] rounded-[7px] bg-primary px-[18px] text-[13px] font-semibold text-primary-foreground"
            >
              Approve &amp; lock <kbd className="font-mono text-[11px]">⌘⏎</kbd>
            </Button>
          </div>
        </footer>
      )}
    </section>
  );
}
