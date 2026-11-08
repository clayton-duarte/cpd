import { ExternalLinkIcon } from "lucide-react";
import type { ReactElement } from "react";
import type { CockpitAction, JobView, PlanViewModel, WorkflowView } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { Empty } from "../components/empty.tsx";
import { ScrollArea } from "../components/scroll-area.tsx";
import { glyphClass, jobDisplay, parseJobStatus, parsePhase, phaseLabel, toneClasses, type Phase, type Tone } from "../dango-status.ts";
import { TopPaneHeader } from "./TopPaneHeader.tsx";
import { layoutDag, NODE_H, NODE_W, type DagNode } from "./plan/layout.ts";

const phaseOrder: readonly Phase[] = ["grounding", "plan", "dispatch", "deploy", "done"];

function Stepper(props: { phase: string }): ReactElement {
  const parsed = parsePhase(props.phase);
  const cur = parsed.kind === "known" ? phaseOrder.indexOf(parsed.value) : -1;
  const items: ReactElement[] = [];
  phaseOrder.forEach((p, i) => {
    if (i > 0) items.push(<li key={`sep-${p}`} aria-hidden="true" data-part="sep" className="text-line">—</li>);
    const state = cur < 0 ? "upcoming" : i < cur ? "past" : i === cur ? "current" : "upcoming";
    items.push(
      <li
        key={p}
        data-phase={p}
        data-state={state}
        aria-current={state === "current" ? "step" : undefined}
        className={cn(
          state === "past" && "px-1.5 py-0.5 text-muted-foreground",
          state === "upcoming" && "px-1.5 py-0.5 text-fg-dim",
          state === "current" && "rounded-[4px] bg-status-active/[.14] px-2 py-0.5 font-semibold text-status-active-bright",
        )}
      >
        {phaseLabel[p]}
      </li>,
    );
  });
  return (
    <>
      <ol aria-label="Phase" data-part="stepper" data-current={props.phase} className="m-0 flex list-none items-center gap-1 p-0 font-mono text-meta">
        {items}
      </ol>
      {parsed.kind === "unknown" ? <span data-part="phase-unknown" className="font-mono text-meta text-fg-dim">{parsed.raw}</span> : null}
    </>
  );
}

function jobText(job: JobView): ReactElement {
  return (
    <>
      {job.needs.length > 0 ? <span data-part="needs" className="sr-only">{` needs: ${job.needs.join(", ")}`}</span> : null}
      {job.attempts.map((a) => (
        <span key={a.id} data-part="attempt" className="sr-only">{` #${a.number} ${a.status}`}</span>
      ))}
    </>
  );
}

function chipClass(status: string): string {
  const p = parseJobStatus(status);
  const s = p.kind === "known" ? p.value : null;
  if (s === "running") return "border border-status-active/55 [&_[data-part=name]]:text-fg-strong";
  if (s === "failed") return "border border-status-blocked [&_[data-part=name]]:text-fg-strong";
  if (s === null || s === "queued" || s === "draft" || s === "cancelled") return "border border-dashed border-line [&_[data-part=name]]:text-muted-foreground";
  return "border border-border [&_[data-part=name]]:text-foreground";
}

function ExploreStrip(props: { jobs: readonly JobView[] }): ReactElement {
  const counts = new Map<string, number>();
  for (const j of props.jobs) counts.set(j.kind, (counts.get(j.kind) ?? 0) + 1);
  const summary = ["explore", ...Array.from(counts, ([k, n]) => `${n} ${k}`)].join(" · ");
  return (
    <section aria-label="Explore jobs" data-part="explore" className="flex flex-wrap items-center gap-2">
      <span data-part="explore-summary" className="mr-1 font-mono text-[11.5px] text-muted-foreground">{summary}</span>
      <ul data-part="chips" className="contents">
        {props.jobs.map((job) => {
          const d = jobDisplay(job.status);
          return (
            <li key={job.key} data-job={job.key} data-status={job.status} data-kind={job.kind}
              className={cn("flex h-6 items-center gap-1.5 rounded-full px-2 text-[11.5px]", chipClass(job.status))}>
              <span aria-hidden="true" className={glyphClass[d.glyph]} />
              <span data-part="name" className="font-mono">{job.key}</span>
              <span data-part="tier" className="font-mono text-fg-dim">{job.tier}</span>
              <span data-part="status" className="sr-only">{d.label}</span>
              {jobText(job)}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

const nodeLook = {
  merged: { border: "border-input", tint: "", sub: "text-status-done", name: "text-foreground" },
  running: { border: "border-status-active", tint: "bg-status-active/[.08]", sub: "text-status-active-bright", name: "text-fg-strong" },
  landing: { border: "border-status-landing", tint: "bg-status-landing/[.08]", sub: "text-status-landing-bright", name: "text-fg-strong" },
  failed: { border: "border-status-blocked", tint: "bg-status-blocked/[.09]", sub: "text-status-blocked", name: "text-fg-strong" },
  idle: { border: "border-dashed border-line", tint: "", sub: "text-muted-foreground", name: "text-foreground" },
} as const;

function lookOf(status: string): (typeof nodeLook)[keyof typeof nodeLook] {
  const p = parseJobStatus(status);
  if (p.kind === "unknown") return nodeLook.idle;
  switch (p.value) {
    case "merged": return nodeLook.merged;
    case "running": return nodeLook.running;
    case "landing": case "checking": case "merging": return nodeLook.landing;
    case "failed": return nodeLook.failed;
    default: return nodeLook.idle;
  }
}

function subtitle(job: JobView, all: readonly JobView[]): string {
  const d = jobDisplay(job.status);
  if (job.status === "failed") {
    const n = job.attempts.filter((a) => a.status === "failed").length;
    if (n >= 2) return `failed ×${n}`;
  }
  if (job.status === "queued") {
    const u = job.needs.filter((k) => all.find((j) => j.key === k)?.status !== "merged").length;
    if (u >= 1) return `blocked on ${u}`;
  }
  return d.label;
}

function Node(props: { node: DagNode; plan: PlanViewModel; wf: WorkflowView; onRetryJob: ((k: string, w: string) => void) | undefined }): ReactElement {
  const { job } = props.node;
  const d = jobDisplay(job.status);
  const look = lookOf(job.status);
  return (
    <li data-job={job.key} data-status={job.status} data-tone={d.tone} data-glyph={d.glyph} data-tier={job.tier} title={job.title}
      style={{ left: props.node.left, top: props.node.top, width: NODE_W, height: NODE_H }}
      className={cn("absolute box-border flex flex-col justify-center gap-0.5 rounded-[6px] border bg-card px-[9px] py-[5px]", look.border)}>
      {look.tint !== "" ? <span aria-hidden="true" data-part="node-tint" className={cn("pointer-events-none absolute inset-0 rounded-[5px]", look.tint)} /> : null}
      <span data-part="node-head" className="relative flex items-center gap-1.5">
        <span aria-hidden="true" className={glyphClass[d.glyph]} />
        <span data-part="name" className={cn("flex-1 truncate font-mono text-[12px]", look.name)}>{job.key}</span>
        <span data-part="tier" className="font-mono text-[10.5px] text-muted-foreground">{job.tier}</span>
      </span>
      <span data-part="node-sub" className="relative flex items-center gap-1.5 font-mono text-meta">
        <span data-part="status" className={cn("flex-1 truncate", look.sub)}>{subtitle(job, props.plan.jobs)}</span>
        {job.status === "failed" && props.onRetryJob !== undefined ? (
          <button type="button" data-action="retry" aria-label={`Retry ${job.key}`}
            className="font-mono text-meta font-semibold text-status-blocked-bright hover:underline"
            onClick={() => props.onRetryJob?.(job.key, props.wf.key)}>
            Retry ▸
          </button>
        ) : null}
      </span>
      {jobText(job)}
    </li>
  );
}

function wfTone(status: string): Tone {
  return status === "open" ? "active" : status === "merged" ? "done" : status === "waiting" ? "decision" : "neutral";
}
const wfToneClass: Readonly<Record<Tone, string>> = {
  active: "text-status-active-bright bg-status-active/[.14]",
  done: "text-status-done bg-status-done/10",
  decision: "text-status-decision-bright bg-status-decision/10",
  neutral: "text-muted-foreground bg-surface-pressed",
  blocked: "", landing: "", focus: "", unread: "",
};

function WorkflowCard(props: { plan: PlanViewModel; wf: WorkflowView; onAction(a: CockpitAction): void; onRetryJob: ((k: string, w: string) => void) | undefined; postableWorkflows: ReadonlySet<string> | undefined }): ReactElement {
  const { plan, wf } = props;
  const layout = layoutDag(wf, plan.jobs);
  const failed = layout.nodes.filter((n) => {
    const p = parseJobStatus(n.job.status);
    return p.kind === "known" && p.value === "failed";
  }).length;
  const tone = wfTone(wf.status);
  const pr = wf.pr;
  const canPost = pr !== null && pr.posted !== true && pr.state !== "merged" && wf.status !== "merged" &&
    (props.postableWorkflows === undefined || props.postableWorkflows.has(wf.key) || props.postableWorkflows.has(pr.url) || props.postableWorkflows.has(`#${pr.number}`));
  return (
    <section data-workflow={wf.key} aria-label={`Workflow ${wf.key}`} data-status={wf.status} className="rounded-[9px] border border-border bg-card">
      <header data-part="wf-header" className="flex h-[34px] flex-wrap items-center gap-2.5 border-b border-border px-3">
        <h3 className="font-mono text-key font-semibold text-fg-strong">{wf.key}</h3>
        <span data-part="wf-route" className="font-mono text-[11.5px] text-muted-foreground">{`${wf.repo} → ${wf.base}`}</span>
        <span data-part="wf-status" data-tone={tone} className={cn("rounded-[4px] px-1.5 py-px font-mono text-meta", wfToneClass[tone])}>{wf.status}</span>
        {failed > 0 ? <span data-part="wf-failed" className="font-mono text-meta text-status-blocked-bright">{`${failed} failed`}</span> : null}
        <span className="flex-1" />
        {wf.pr !== null ? (
          <p data-part="pr" className="flex items-center gap-2 font-mono text-[11.5px]">
            <a href={wf.pr.url} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-status-focus-bright underline-offset-2 hover:underline">
              {`#${wf.pr.number} ${wf.pr.state ?? ""}`.trim()}
              <ExternalLinkIcon aria-hidden="true" className="size-3" />
            </a>
            {canPost ? (
              <Button type="button" variant="outline" size="xs" data-action="post-pr"
                onClick={() => props.onAction({ kind: "pr.post", planId: plan.planId, workflow: wf.key })}>
                Post PR
              </Button>
            ) : null}
          </p>
        ) : null}
      </header>
      <ScrollArea data-part="dag-body" className="w-full">
      <div className="w-fit p-3">
        {layout.nodes.length === 0 ? (
          <p data-part="no-jobs" className="text-[11.5px] text-muted-foreground">No jobs</p>
        ) : (
          <div data-part="dag-canvas" className="relative" style={{ width: layout.width, height: layout.height }}>
            <svg data-part="dag-edges" aria-hidden="true" className="pointer-events-none absolute left-0 top-0"
              width={layout.width} height={layout.height} viewBox={`0 0 ${layout.width} ${layout.height}`} fill="none">
              {layout.edges.map((e) => (
                <path key={e.id} data-edge={e.id} data-state={e.state} d={e.d} strokeWidth={1.5}
                  className={e.state === "done" ? "stroke-status-done" : "stroke-line"}
                  strokeOpacity={e.state === "done" ? 0.6 : undefined}
                  strokeDasharray={e.state === "done" ? undefined : "3 3"} />
              ))}
            </svg>
            <ul data-part="dag" className="absolute inset-0 m-0 list-none p-0">
              {layout.nodes.map((n) => (
                <Node key={n.job.key} node={n} plan={plan} wf={wf} onRetryJob={props.onRetryJob} />
              ))}
            </ul>
          </div>
        )}
      </div>
      </ScrollArea>
    </section>
  );
}

function ReviewPrCard(props: { pr: NonNullable<PlanViewModel["reviewedPr"]> }): ReactElement {
  const { pr } = props;
  return (
    <section aria-label="Review PR" data-part="review-pr" className="rounded-[9px] border border-border bg-card px-3 py-2">
      <h3 className="font-mono text-key font-semibold text-fg-strong">Review PR</h3>
      <p data-part="review-pr-repo" className="font-mono text-[11.5px] text-muted-foreground">{pr.repo}</p>
      <p data-part="review-pr-title" className="text-foreground">{pr.title}</p>
      <p data-part="pr" className="font-mono text-[11.5px]">
        <a href={pr.url} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-status-focus-bright underline-offset-2 hover:underline">
          {`#${pr.number}${pr.state === null ? "" : ` ${pr.state}`}`}
          <ExternalLinkIcon aria-hidden="true" className="size-3" />
        </a>
      </p>
    </section>
  );
}

export function PlanView(props: { plan: PlanViewModel | null; onAction(action: CockpitAction): void;
  onRetryJob?: (jobKey: string, workflowKey: string) => void;
  postableWorkflows?: ReadonlySet<string>;
  onSelectManager?: () => void;
}): ReactElement {
  const { plan, onAction } = props;
  if (plan === null) {
    // 14.2 (air-d28): the empty state is text, not the flowchart; CpdFlowCanvas renders only in the Documentation dialog.
    return <Empty data-part="plan-empty" className="h-full text-muted-foreground">Select a lead to see plans</Empty>;
  }
  const explore = plan.jobs.filter((j) => j.workflow === null);
  return (
    <section aria-label={`Plan ${plan.planId}`} data-phase={plan.phase} className="flex h-full min-h-0 w-full min-w-0 flex-col overflow-hidden">
      <TopPaneHeader
        dataPart="title-row"
        actions={
          <>
            {plan.review !== null ? <p data-part="review" className="font-mono text-meta text-muted-foreground">{`Review ${plan.review.id}: ${plan.review.status}`}</p> : null}
            {plan.openEscalations > 0 ? <p data-part="escalations" className="font-mono text-meta text-status-blocked">{`Escalations: ${plan.openEscalations}`}</p> : null}
          </>
        }
      >
        {props.onSelectManager !== undefined ? (
          <button type="button" data-action="select-manager" onClick={props.onSelectManager}
            className="shrink-0 font-mono text-[14px] font-semibold text-fg-strong hover:underline">
            {plan.contextId}
          </button>
        ) : (
          <span data-part="manager-key" className="shrink-0 font-mono text-[14px] font-semibold text-fg-strong">{plan.contextId}</span>
        )}
        <span aria-hidden="true" className="text-fg-dim">/</span>
        <h2 data-part="plan-key" title={plan.title} className="min-w-0 truncate font-mono text-[14px] font-semibold text-fg-strong">{plan.planId}</h2>
        <Stepper phase={plan.phase} />
      </TopPaneHeader>
      <ScrollArea data-part="plan-scroll" className="min-h-0 flex-1 [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:w-full [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:!min-w-0">
      <div data-part="plan-body" className="flex flex-col gap-2.5 px-4 py-2.5">
        {explore.length > 0 ? <ExploreStrip jobs={explore} /> : null}
        {plan.workflows.map((wf) => (
          <WorkflowCard key={wf.key} plan={plan} wf={wf} onAction={onAction} onRetryJob={props.onRetryJob} postableWorkflows={props.postableWorkflows} />
        ))}
        {plan.reviewedPr !== null && plan.reviewedPr !== undefined && plan.workflows.length === 0 ? <ReviewPrCard pr={plan.reviewedPr} /> : null}
        {plan.workflows.length === 0 && explore.length === 0 && (plan.reviewedPr ?? null) === null ? <p data-part="no-work" className="px-1 text-muted-foreground">No workflows yet</p> : null}
      </div>
      </ScrollArea>
    </section>
  );
}
