import { PlusIcon } from "lucide-react";
import { Fragment } from "react";
import type { ReactElement } from "react";
import type { ContextNode, ContextTreeModel, PlanCounts, PlanNode, SessionNode } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { dotDisplay, glyphClass, planState, sessionDisplay } from "../dango-status.ts";

const COUNT_ORDER: readonly (keyof PlanCounts)[] = ["draft", "queued", "merged", "running", "failed", "cancelled"];
const COUNT_TONE: Record<keyof PlanCounts, string> = {
  draft: "text-muted-foreground",
  queued: "text-muted-foreground",
  merged: "text-status-done",
  running: "text-status-active",
  failed: "text-status-blocked",
  cancelled: "text-muted-foreground",
};
const SELECTED = "bg-sidebar-accent text-fg-strong";

interface Handlers {
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
}

function Counts({ counts }: { counts: PlanCounts }): ReactElement {
  const shown = COUNT_ORDER.filter((k) => counts[k] > 0);
  return (
    <span data-part="counts" className={cn("min-w-[52px] text-right font-mono text-meta", shown.length === 0 && "text-fg-dim")}>
      {shown.map((k, i) => (
        <Fragment key={k}>
          {i > 0 ? <span className="text-line"> · </span> : null}
          <span data-count={k} className={COUNT_TONE[k]}>{`${k} ${counts[k]}`}</span>
        </Fragment>
      ))}
    </span>
  );
}

function Aggregate({ sessions }: { sessions: readonly SessionNode[] }): ReactElement {
  const key = planState(sessions.map((s) => dotDisplay(s.dot)));
  const d = sessionDisplay[key];
  return (
    <>
      <span data-part="aggregate" data-tone={d.tone} data-glyph={d.glyph} aria-hidden="true" className={glyphClass[d.glyph]} />
      <span className="sr-only">{d.label}</span>
    </>
  );
}

function SessionRow(props: { node: SessionNode } & Handlers): ReactElement {
  const { node } = props;
  const { state, attention } = node.dot;
  const label = attention === null ? state : `${state}, needs you (${attention})`;
  const d = sessionDisplay[dotDisplay(node.dot)];
  const current = props.selectedSessionId === node.ref.id;
  return (
    <li>
      <span data-part="session-row" className={cn("flex h-[26px] items-center gap-2 rounded-sm pl-8 pr-2", current && SELECTED)}>
        <span
          role="img"
          data-dot={state}
          data-attention={attention ?? "none"}
          data-tone={d.tone}
          data-glyph={d.glyph}
          aria-label={label}
          className={glyphClass[d.glyph]}
        />
        <button
          type="button"
          data-session={node.ref.id}
          aria-current={current ? "true" : undefined}
          onClick={() => props.onSelectSession(node.ref.id)}
          className="min-w-0 flex-1 truncate text-left"
        >
          <span className="font-mono">{node.ref.key}</span>
          {node.ref.title === null ? null : <span className="text-muted-foreground">{` — ${node.ref.title}`}</span>}
        </button>
      </span>
    </li>
  );
}

function PlanItem(props: { plan: PlanNode } & Handlers): ReactElement {
  const { plan } = props;
  const agg = planState(plan.sessions.map((s) => dotDisplay(s.dot)));
  const current = props.selectedPlanId === plan.id;
  const quiet = agg === "idle" || agg === "asleep";
  return (
    <li role="treeitem" data-plan={plan.id}>
      <div data-part="plan-row" className="flex h-[26px] items-center gap-2 rounded-sm pl-8 pr-2">
        <Aggregate sessions={plan.sessions} />
        <button
          type="button"
          data-select="plan"
          aria-current={current ? "true" : undefined}
          onClick={() => props.onSelectPlan(plan.id)}
          className={cn(
            "min-w-0 flex-1 truncate rounded-sm text-left font-mono text-key",
            quiet ? "text-muted-foreground" : "text-foreground",
            current ? SELECTED : "hover:bg-row-hover",
          )}
        >
          {plan.title}
        </button>
        <Counts counts={plan.counts} />
      </div>
      <ul>
        {plan.sessions.map((s) => (
          <SessionRow key={s.ref.id} node={s} {...props} />
        ))}
      </ul>
    </li>
  );
}

function ContextItem(props: { context: ContextNode } & Handlers): ReactElement {
  const { context } = props;
  const live = context.plans.filter((p) => !p.archived);
  const archived = context.plans.filter((p) => p.archived);
  const planItems = (plans: readonly PlanNode[]) => plans.map((p) => <PlanItem key={p.id} plan={p} {...props} />);
  const aggSessions = [...context.sessions, ...live.flatMap((p) => p.sessions)];
  return (
    <li role="treeitem" data-context={context.id} className="mb-1">
      <div data-part="context-row" className="flex h-7 items-center gap-2 rounded-sm px-2">
        <span aria-hidden="true" className="w-[10px] font-mono text-[10px] text-fg-dim">▾</span>
        <Aggregate sessions={aggSessions} />
        <span className="flex-1 truncate font-mono text-key font-semibold text-fg-strong">{context.title}</span>
      </div>
      <ul role="group">
        {planItems(live)}
        {archived.length > 0 ? (
          <li>
            <details data-archived>
              <summary className="flex h-7 items-center gap-2 px-2 text-muted-foreground">Archived ({archived.length})</summary>
              <ul role="group">{planItems(archived)}</ul>
            </details>
          </li>
        ) : null}
      </ul>
      <ul>
        {context.sessions.map((s) => (
          <SessionRow key={s.ref.id} node={s} {...props} />
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
  const items = (cs: readonly ContextNode[]) => cs.map((c) => <ContextItem key={c.id} context={c} {...handlers} />);
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
      {tree === null ? null : <ul role="tree" className="flex flex-col gap-0.5 px-1.5 pb-2.5">{items(live)}</ul>}
      {archived.length > 0 ? (
        <details data-archived>
          <summary className="mx-1.5 flex h-7 items-center gap-2 px-2 text-muted-foreground">Archived ({archived.length})</summary>
          <ul role="tree">{items(archived)}</ul>
        </details>
      ) : null}
    </nav>
  );
}
