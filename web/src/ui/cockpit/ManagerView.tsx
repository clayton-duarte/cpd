import { ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon } from "lucide-react";
import { useState, type ReactElement } from "react";
import type { ContextNode, PlanNode, PlanWorkflowSummary } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { STOPPED, compareOrderKey, createdOf, leadOf } from "../../state/archiveNav.ts";
import { Badge } from "../components/badge.tsx";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../components/collapsible.tsx";
import { TopPaneHeader } from "./TopPaneHeader.tsx";
import { Empty, EmptyHeader, EmptyTitle } from "../components/empty.tsx";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "../components/hover-card.tsx";
import { Dot } from "./ContextTree.tsx";
import { CpdFlowCanvas } from "./FlowCanvas.tsx";

function phaseTone(phase: string): string {
  if (phase === "shaping" || phase === "grounding") {
    return "border-sakura/50 bg-sakura/12 text-sakura";
  }
  if (phase === "plan") {
    return "border-shiro/50 bg-shiro/12 text-shiro";
  }
  return "border-matcha/50 bg-matcha/12 text-matcha";
}

function activeAgentOf(plan: PlanNode): { label: "manager" | "lead" | "worker"; className: string } {
  if (plan.counts.running > 0) {
    return {
      label: "worker",
      className: "border-matcha-bright/60 bg-matcha-bright/15 text-matcha-bright",
    };
  }
  if (plan.phase === "shaping" || plan.phase === "done" || plan.leadSessionId === null) {
    return {
      label: "manager",
      className: "border-sakura-bright/60 bg-sakura-bright/15 text-sakura-bright",
    };
  }
  return {
    label: "lead",
    className: "border-shiro-bright/60 bg-shiro-bright/15 text-shiro-bright",
  };
}

function sortPlans(plans: readonly PlanNode[]): PlanNode[] {
  return [...plans].sort((a, b) =>
    compareOrderKey(leadOf(a)?.dot ?? STOPPED, createdOf(a), leadOf(b)?.dot ?? STOPPED, createdOf(b)),
  );
}

function WorkflowChip({ wf }: { wf: PlanWorkflowSummary }): ReactElement {
  if (wf.pr !== null) {
    const state = wf.pr.state ?? wf.status;
    const checks = wf.pr.checks;
    const isMerged = state === "merged" || wf.status === "merged";
    const isFailed = checks === "failed" || wf.pr.mergeable === "conflicting";
    const tone = isMerged
      ? "border-matcha/40 bg-matcha/10 text-matcha-bright"
      : isFailed
        ? "border-anko/50 bg-anko/12 text-anko-bright"
        : "border-mizu/45 bg-mizu/10 text-mizu-bright";
    const suffix = !isMerged && checks !== null && checks !== "none" ? ` · ${checks}` : "";
    return (
      <a
        href={wf.pr.url}
        target="_blank"
        rel="noopener noreferrer"
        data-part="wf-pr-chip"
        onClick={(e) => e.stopPropagation()}
        className={cn(
          "inline-flex h-4 items-center gap-1 rounded-[4px] border px-1.5 font-mono text-[10px] hover:underline",
          tone,
        )}
        title={`${wf.key}: #${wf.pr.number} ${state}${suffix}`}
      >
        <GitPullRequestIcon aria-hidden="true" className="size-2.5 shrink-0" />
        <span>{`#${wf.pr.number} ${state}${suffix}`}</span>
      </a>
    );
  }
  return (
    <span
      data-part="wf-chip"
      className="inline-flex h-4 items-center rounded-[4px] border border-border/70 bg-background/60 px-1.5 font-mono text-[10px] text-muted-foreground"
    >
      {`${wf.key} (${wf.status})`}
    </span>
  );
}

function WorkflowsDisplay({ wfs }: { wfs: readonly PlanWorkflowSummary[] }): ReactElement | null {
  const [open, setOpen] = useState(false);
  if (wfs.length === 0) return null;
  if (wfs.length <= 2) {
    return (
      <span data-part="plan-workflows" className="flex shrink-0 items-center gap-1">
        {wfs.map((wf) => (
          <WorkflowChip key={wf.key} wf={wf} />
        ))}
      </span>
    );
  }
  const hasFailed = wfs.some((w) => w.pr?.checks === "failed" || w.pr?.mergeable === "conflicting");
  const allMerged = wfs.every((w) => (w.pr?.state ?? w.status) === "merged" || w.status === "merged");
  const tone = hasFailed
    ? "border-anko/50 bg-anko/12 text-anko-bright"
    : allMerged
      ? "border-matcha/40 bg-matcha/10 text-matcha-bright"
      : "border-mizu/45 bg-mizu/10 text-mizu-bright";
  return (
    <span data-part="plan-workflows" className="flex shrink-0 items-center" onClick={(e) => e.stopPropagation()}>
      <HoverCard open={open} onOpenChange={setOpen} openDelay={10} closeDelay={100}>
        <HoverCardTrigger asChild>
          <button
            type="button"
            data-part="wf-pr-summary"
            onFocus={() => setOpen(true)}
            className={cn(
              "inline-flex h-4 cursor-pointer items-center gap-1 rounded-[4px] border px-1.5 font-mono text-[10px]",
              tone,
            )}
          >
            <GitPullRequestIcon aria-hidden="true" className="size-2.5 shrink-0" />
            <span>{`${wfs.length} PRs`}</span>
            <ChevronDownIcon aria-hidden="true" className="size-2.5 shrink-0 opacity-70" />
          </button>
        </HoverCardTrigger>
        <HoverCardContent
          align="end"
          data-part="wf-pr-hover-list"
          className="flex w-auto min-w-40 flex-col gap-1 p-2"
        >
          {wfs.map((wf) => (
            <WorkflowChip key={wf.key} wf={wf} />
          ))}
        </HoverCardContent>
      </HoverCard>
    </span>
  );
}

function JobCountersPill({ plan }: { plan: PlanNode }): ReactElement {
  const planned = plan.counts.draft + plan.counts.queued;
  const active = plan.counts.running + (plan.counts.merging ?? 0);
  const failed = plan.counts.failed;
  const done = plan.counts.merged;
  return (
    <span
      data-part="plan-counters"
      title={`Jobs — planned (draft+queued): ${planned} · active: ${active} · failed: ${failed} · merged: ${done}`}
      className="inline-flex h-4 shrink-0 items-center gap-0.5 rounded-[4px] bg-surface-pressed px-1.5 font-mono text-[10px] text-fg-dim"
    >
      <span className={planned > 0 ? "text-foreground" : undefined}>{planned}</span>
      <span>·</span>
      <span className={active > 0 ? "font-semibold text-mizu-bright" : undefined}>{active}</span>
      <span>·</span>
      <span className={failed > 0 ? "font-semibold text-anko-bright" : undefined}>{failed}</span>
      <span>·</span>
      <span className={done > 0 ? "text-matcha-bright" : undefined}>{done}</span>
    </span>
  );
}

function PlanRowCard(props: {
  plan: PlanNode;
  onSelect: (plan: PlanNode) => void;
}): ReactElement {
  const { plan, onSelect } = props;
  const lead = leadOf(plan);
  const agent = activeAgentOf(plan);
  const wfs = (plan.workflows ?? []).filter((w) => w.status !== "cancelled");
  return (
    <li data-manager-plan={plan.id} data-phase={plan.phase} className="min-w-0">
      <div
        role="button"
        tabIndex={0}
        onClick={() => onSelect(plan)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect(plan);
          }
        }}
        className="group flex w-full min-w-0 cursor-pointer items-center gap-2 overflow-hidden rounded-[6px] border border-border bg-card px-2.5 py-1.5 text-left transition-colors hover:border-ring/60 hover:bg-row-hover"
      >
        <Dot dot={lead === undefined ? STOPPED : lead.dot} plan={plan} archived={plan.archived} />
        <span
          data-part="plan-id"
          className="shrink-0 font-mono text-[12px] font-semibold text-fg-strong"
        >
          {plan.id}
        </span>
        <span
          data-part="plan-title"
          title={plan.title}
          className="min-w-0 flex-1 truncate text-[12px] text-muted-foreground"
        >
          {plan.title}
        </span>

        <WorkflowsDisplay wfs={wfs} />

        <Badge
          variant="outline"
          data-part="plan-phase"
          className={cn("h-4 shrink-0 px-1.5 py-0 font-mono text-[10px]", phaseTone(plan.phase))}
        >
          {plan.phase}
        </Badge>
        <Badge
          variant="outline"
          data-part="plan-actor"
          className={cn("h-4 shrink-0 px-1.5 py-0 font-mono text-[10px]", agent.className)}
        >
          {agent.label}
        </Badge>

        <JobCountersPill plan={plan} />
      </div>
    </li>
  );
}

export interface ManagerViewProps {
  readonly context: ContextNode | null;
  readonly onSelectPlan: (planId: string) => void;
  readonly onSelectSession: (sessionId: string) => void;
}

export function ManagerView(props: ManagerViewProps): ReactElement {
  const { context, onSelectPlan, onSelectSession } = props;
  const [doneOpen, setDoneOpen] = useState(false);

  if (context === null) {
    return <CpdFlowCanvas />;
  }

  const unarchived = context.plans.filter((p) => !p.archived);
  const activePlans = sortPlans(unarchived.filter((p) => p.phase !== "done"));
  const donePlans = sortPlans(unarchived.filter((p) => p.phase === "done"));

  const handleSelectPlan = (plan: PlanNode): void => {
    onSelectPlan(plan.id);
    if (plan.leadSessionId !== null) {
      onSelectSession(plan.leadSessionId);
    }
  };

  return (
    <section
      aria-label={`Manager overview ${context.id}`}
      data-manager-view={context.id}
      className="flex min-w-0 flex-col overflow-x-hidden"
    >
      <TopPaneHeader dataPart="manager-header">
        <h2
          data-part="manager-key"
          title={context.title}
          className="shrink-0 font-mono text-[14px] font-semibold text-fg-strong"
        >
          {context.id}
        </h2>
        <span data-part="manager-counters" className="min-w-0 truncate font-mono text-meta text-muted-foreground">
          {`${activePlans.length} active · ${donePlans.length} done · ${context.records} records`}
        </span>
      </TopPaneHeader>

      <div data-part="manager-body" className="flex min-w-0 flex-col gap-2 px-4 py-2.5">
        {activePlans.length === 0 && donePlans.length === 0 ? (
          <Empty data-empty className="border-none py-8 text-muted-foreground">
            <EmptyHeader>
              <EmptyTitle className="text-xs font-normal text-muted-foreground">
                No plans in this context yet — chat with the Manager below to shape a Plan
              </EmptyTitle>
            </EmptyHeader>
          </Empty>
        ) : (
          <>
            {activePlans.length > 0 ? (
              <ol data-part="active-plans" className="m-0 flex list-none flex-col gap-1.5 p-0">
                {activePlans.map((p) => (
                  <PlanRowCard key={p.id} plan={p} onSelect={handleSelectPlan} />
                ))}
              </ol>
            ) : (
              <p className="px-1 font-mono text-xs text-muted-foreground">
                All current plans are done.
              </p>
            )}

            {donePlans.length > 0 ? (
              <Collapsible open={doneOpen} onOpenChange={setDoneOpen} className="mt-1">
                <CollapsibleTrigger
                  type="button"
                  className="group flex w-full cursor-pointer items-center gap-1.5 rounded-[5px] px-1.5 py-1 text-left font-mono text-[11px] text-muted-foreground hover:bg-row-hover hover:text-fg-strong"
                >
                  <ChevronRightIcon
                    aria-hidden="true"
                    className="size-3.5 shrink-0 transition-transform duration-200 group-data-[state=open]:rotate-90"
                  />
                  <span>{`Done plans (${donePlans.length})`}</span>
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <ol data-part="done-plans" className="m-0 mt-1.5 flex list-none flex-col gap-1.5 p-0">
                    {donePlans.map((p) => (
                      <PlanRowCard key={p.id} plan={p} onSelect={handleSelectPlan} />
                    ))}
                  </ol>
                </CollapsibleContent>
              </Collapsible>
            ) : null}
          </>
        )}
      </div>
    </section>
  );
}
