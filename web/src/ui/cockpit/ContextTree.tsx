import type { ReactElement } from "react";
import type { ContextNode, ContextTreeModel, PlanCounts, PlanNode, SessionNode } from "../../protocol/cockpit.ts";

const COUNT_ORDER: readonly (keyof PlanCounts)[] = ["draft", "queued", "merged", "running", "failed", "cancelled"];

function countsText(counts: PlanCounts): string {
  return COUNT_ORDER.filter((k) => counts[k] > 0)
    .map((k) => `${k} ${counts[k]}`)
    .join(" · ");
}

interface Handlers {
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
}

function SessionRow(props: { node: SessionNode } & Handlers): ReactElement {
  const { node } = props;
  const { state, attention } = node.dot;
  const label = attention === null ? state : `${state}, needs you (${attention})`;
  return (
    <li>
      <span>
        <span role="img" data-dot={state} data-attention={attention ?? "none"} aria-label={label} />
        <button
          type="button"
          data-session={node.ref.id}
          aria-current={props.selectedSessionId === node.ref.id ? "true" : undefined}
          onClick={() => props.onSelectSession(node.ref.id)}
        >
          {node.ref.title === null ? node.ref.key : `${node.ref.key} — ${node.ref.title}`}
        </button>
      </span>
    </li>
  );
}

function PlanItem(props: { plan: PlanNode } & Handlers): ReactElement {
  const { plan } = props;
  return (
    <li role="treeitem" data-plan={plan.id}>
      <button
        type="button"
        data-select="plan"
        aria-current={props.selectedPlanId === plan.id ? "true" : undefined}
        onClick={() => props.onSelectPlan(plan.id)}
      >
        {plan.title}
      </button>{" "}
      <span>{countsText(plan.counts)}</span>
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
  return (
    <li role="treeitem" data-context={context.id}>
      <span>{context.title}</span>
      <ul role="group">
        {planItems(live)}
        {archived.length > 0 ? (
          <li>
            <details data-archived>
              <summary>Archived ({archived.length})</summary>
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
  const { tree, onNewContext: _onNewContext, ...handlers } = props;
  if (tree === null) return <nav aria-label="Context tree">Loading contexts…</nav>;
  const live = tree.contexts.filter((c) => !c.archived);
  const archived = tree.contexts.filter((c) => c.archived);
  const items = (cs: readonly ContextNode[]) => cs.map((c) => <ContextItem key={c.id} context={c} {...handlers} />);
  return (
    <nav aria-label="Context tree">
      <ul role="tree">{items(live)}</ul>
      {archived.length > 0 ? (
        <details data-archived>
          <summary>Archived ({archived.length})</summary>
          <ul role="tree">{items(archived)}</ul>
        </details>
      ) : null}
    </nav>
  );
}
