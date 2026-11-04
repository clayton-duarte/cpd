import { PlusIcon } from "lucide-react";
import { Fragment } from "react";
import type { ReactElement } from "react";
import type { ContextNode, ContextTreeModel, PlanCounts, PlanNode, SessionNode } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { dotDisplay, glyphClass, planState, sessionDisplay } from "../dango-status.ts";

const SELECTED = "bg-sidebar-accent text-fg-strong";
const COUNTERS = [
  ["planned", "text-muted-foreground"],
  ["active", "text-status-active"],
  ["failed", "text-status-blocked"],
  ["done", "text-status-done"],
] as const;

interface Handlers {
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
}

function Counters({ plans }: { plans: readonly PlanNode[] }): ReactElement {
  const sum = (f: (c: PlanCounts) => number) => plans.reduce((n, p) => n + f(p.counts), 0);
  const value = {
    planned: sum((c) => c.draft + c.queued),
    active: sum((c) => c.running),
    failed: sum((c) => c.failed),
    done: sum((c) => c.merged),
  };
  return (
    <span data-part="counters" className="font-mono text-meta">
      {COUNTERS.map(([k, tone], i) => (
        <Fragment key={k}>
          {i > 0 ? <span className="text-line"> · </span> : null}
          <span data-count={k} className={tone}>{String(value[k])}</span>
        </Fragment>
      ))}
    </span>
  );
}

const STOPPED: SessionNode["dot"] = { state: "stopped", attention: null };

function Dot({ dot }: { dot: SessionNode["dot"] }): ReactElement {
  const { state, attention } = dot;
  const label = attention === null ? state : `${state}, needs you (${attention})`;
  const d = sessionDisplay[dotDisplay(dot)];
  return (
    <span role="img" data-dot={state} data-attention={attention ?? "none"} data-tone={d.tone} data-glyph={d.glyph} aria-label={label} className={glyphClass[d.glyph]} />
  );
}

function ForkRow(props: { node: SessionNode; label: string } & Handlers): ReactElement {
  const { node } = props;
  const current = props.selectedSessionId === node.ref.id;
  return (
    <li>
      <span data-part="session-row" className={cn("flex h-[26px] items-center gap-2 rounded-sm pl-8 pr-2", current && SELECTED)}>
        <Dot dot={node.dot} />
        <button
          type="button"
          data-session={node.ref.id}
          aria-current={current ? "true" : undefined}
          onClick={() => props.onSelectSession(node.ref.id)}
          className="min-w-0 flex-1 truncate text-left font-mono"
        >
          {props.label}
        </button>
      </span>
    </li>
  );
}

const isFork = (s: SessionNode): boolean => s.ref.role === "lead" && !s.archived;

function PlanItem(props: { plan: PlanNode } & Handlers): ReactElement {
  const { plan } = props;
  const lead = plan.sessions.find((s) => s.ref.id === plan.leadSessionId);
  const forks = plan.sessions.filter((s) => isFork(s) && s.ref.id !== plan.leadSessionId);
  const current = props.selectedPlanId === plan.id || (lead !== undefined && props.selectedSessionId === lead.ref.id);
  return (
    <li role="treeitem" data-plan={plan.id}>
      <div data-part="plan-row" className={cn("flex h-[26px] items-center rounded-sm pl-6 pr-2", current && SELECTED)}>
        <button
          type="button"
          data-select="plan"
          aria-current={current ? "true" : undefined}
          onClick={() => {
            props.onSelectPlan(plan.id);
            if (lead !== undefined) props.onSelectSession(lead.ref.id);
          }}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <Dot dot={lead === undefined ? STOPPED : lead.dot} />
          <span data-part="plan-id" title={plan.title} className="min-w-0 flex-1 truncate font-mono text-key">{plan.id}</span>
          <Counters plans={[plan]} />
        </button>
      </div>
      {forks.length > 0 ? (
        <ul>
          {forks.map((s) => (
            <ForkRow key={s.ref.id} node={s} label={`${plan.id}·${s.ref.id.slice(0, 4)}`} {...props} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

function ContextItem(props: { context: ContextNode; allPlans: boolean } & Handlers): ReactElement {
  const { context } = props;
  const plans = props.allPlans ? context.plans : context.plans.filter((p) => !p.archived);
  const manager = context.sessions.find((s) => s.ref.role === "manager" && !s.archived);
  const forks = context.sessions.filter(isFork);
  const current = manager !== undefined && props.selectedSessionId === manager.ref.id;
  return (
    <li role="treeitem" data-context={context.id} className="mb-1">
      <button
        type="button"
        data-select="context"
        aria-current={current ? "true" : undefined}
        onClick={() => {
          if (manager !== undefined) props.onSelectSession(manager.ref.id);
        }}
        className={cn("flex h-7 w-full items-center gap-2 rounded-sm px-2 text-left", current ? SELECTED : "hover:bg-row-hover")}
      >
        {props.allPlans ? null : <Dot dot={manager === undefined ? STOPPED : manager.dot} />}
        <span data-part="context-id" title={context.title} className="min-w-0 flex-1 truncate font-mono text-key font-semibold text-fg-strong">{context.id}</span>
        <Counters plans={plans} />
      </button>
      <ul role="group">
        {plans.map((p) => (
          <PlanItem key={p.id} plan={p} {...props} />
        ))}
        {forks.map((s) => (
          <ForkRow key={s.ref.id} node={s} label={s.ref.key} {...props} />
        ))}
      </ul>
    </li>
  );
}

export function ContextTree(props: {
  tree: ContextTreeModel | null;
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
  onNewContext?: () => void;
}): ReactElement {
  const { tree, onNewContext, ...handlers } = props;
  const live = tree === null ? [] : tree.contexts.filter((c) => !c.archived);
  const archived = tree === null ? [] : tree.contexts.filter((c) => c.archived);
  const archivedPlans = live.flatMap((c) => c.plans.filter((p) => p.archived).map((p) => ({ c, p })));
  const archivedCount = archived.length + archived.reduce((n, c) => n + c.plans.filter((p) => p.archived).length, 0) + archivedPlans.length;
  return (
    <nav aria-label="Context tree" className="flex min-h-full flex-col">
      <div data-part="tree-header" className="sticky top-0 z-10 flex h-9 items-center gap-2 bg-background px-[14px]">
        <span className="text-[11px] font-semibold uppercase tracking-[.08em] text-muted-foreground">Contexts</span>
        <span className="flex-1" />
        {onNewContext === undefined ? null : (
          <button
            type="button"
            data-action="new-context"
            aria-label="New context"
            onClick={onNewContext}
            className="grid size-6 place-items-center rounded-[5px] text-fg-strong"
          >
            <PlusIcon size={14} aria-hidden="true" />
          </button>
        )}
      </div>
      {tree === null ? (
        <p data-part="loading" className="px-[14px] text-fg-dim">Loading contexts…</p>
      ) : live.length === 0 && archived.length === 0 ? (
        <p data-part="empty" className="px-[14px] text-fg-dim">No contexts</p>
      ) : null}
      {tree === null ? null : <ul role="tree" className="flex flex-col gap-0.5 px-1.5 pb-2.5">{live.map((c) => <ContextItem key={c.id} context={c} allPlans={false} {...handlers} />)}</ul>}
      {archivedCount > 0 ? (
        <details data-archived>
          <summary className="mx-1.5 flex h-7 items-center gap-2 px-2 text-muted-foreground">{`Archived (${archivedCount})`}</summary>
          <ul role="tree">
            {archived.map((c) => <ContextItem key={c.id} context={c} allPlans {...handlers} />)}
            {archivedPlans.map(({ c, p }) => (
              <PlanItem key={`${c.id}/${p.id}`} plan={p} {...handlers} />
            ))}
          </ul>
        </details>
      ) : null}
    </nav>
  );
}
