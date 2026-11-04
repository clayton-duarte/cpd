import { ChevronRightIcon, PlusIcon } from "lucide-react";
import { Fragment, useState } from "react";
import type { ReactElement } from "react";
import type { ContextNode, ContextTreeModel, PlanCounts, PlanNode, SessionNode } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { dotDisplay, glyphClass, sessionDisplay } from "../dango-status.ts";
import {
  Sidebar, SidebarContent, SidebarGroup, SidebarGroupAction, SidebarGroupContent, SidebarGroupLabel,
  SidebarMenu, SidebarMenuAction, SidebarMenuButton, SidebarMenuItem, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem,
} from "../components/sidebar.tsx";

const SELECTED = "bg-sidebar-accent text-fg-strong";
const COUNTERS = [
  ["draft", "text-status-decision"],
  ["queued", "text-muted-foreground"],
  ["running", "text-status-active"],
  ["merging", "text-status-landing"],
  ["failed", "text-status-blocked"],
  ["merged", "text-status-done"],
] as const;

interface Handlers {
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
}

function Counters({ counts }: { counts: PlanCounts }): ReactElement | null {
  const shown = COUNTERS.filter(([k]) => (counts[k] ?? 0) > 0);
  if (shown.length === 0) return null;
  return (
    <span data-part="counters" className="font-mono text-meta">
      {shown.map(([k, tone], i) => (
        <Fragment key={k}>
          {i > 0 ? " " : null}
          <span data-count={k} className={tone}>{String(counts[k])}</span>
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
    <SidebarMenuSubItem>
      <SidebarMenuSubButton
        data-session={node.ref.id}
        aria-current={current ? "true" : undefined}
        onClick={() => props.onSelectSession(node.ref.id)}
        className={cn("font-mono", current && SELECTED)}
      >
        <Dot dot={node.dot} />
        <span className="min-w-0 flex-1 truncate">{props.label}</span>
      </SidebarMenuSubButton>
    </SidebarMenuSubItem>
  );
}

const isFork = (s: SessionNode): boolean => s.ref.role === "lead" && !s.archived;

function PlanItem(props: { plan: PlanNode } & Handlers): ReactElement {
  const { plan } = props;
  const lead = plan.sessions.find((s) => s.ref.id === plan.leadSessionId);
  const forks = plan.sessions.filter((s) => isFork(s) && s.ref.id !== plan.leadSessionId);
  const current = props.selectedPlanId === plan.id || (lead !== undefined && props.selectedSessionId === lead.ref.id);
  return (
    <>
      <SidebarMenuSubItem role="treeitem" data-plan={plan.id}>
        <SidebarMenuSubButton
          data-select="plan"
          aria-current={current ? "true" : undefined}
          onClick={() => {
            props.onSelectPlan(plan.id);
            if (lead !== undefined) props.onSelectSession(lead.ref.id);
          }}
          className={cn(current && SELECTED)}
        >
          <Dot dot={lead === undefined ? STOPPED : lead.dot} />
          <span data-part="plan-id" title={plan.title} className="min-w-0 flex-1 truncate font-mono text-key">{plan.id}</span>
          <Counters counts={plan.counts} />
        </SidebarMenuSubButton>
      </SidebarMenuSubItem>
      {forks.map((s) => (
        <ForkRow key={s.ref.id} node={s} label={`${plan.id}·${s.ref.id.slice(0, 4)}`} {...props} />
      ))}
    </>
  );
}

function ContextItem(props: { context: ContextNode; allPlans: boolean } & Handlers): ReactElement {
  const { context } = props;
  const [open, setOpen] = useState(true);
  const plans = props.allPlans ? context.plans : context.plans.filter((p) => !p.archived);
  const manager = context.sessions.find((s) => s.ref.role === "manager" && !s.archived);
  const forks = context.sessions.filter(isFork);
  const current = manager !== undefined && props.selectedSessionId === manager.ref.id;
  return (
    <SidebarMenuItem role="treeitem" data-context={context.id}>
      <SidebarMenuButton
        data-select="context"
        aria-current={current ? "true" : undefined}
        onClick={() => {
          if (manager !== undefined) props.onSelectSession(manager.ref.id);
        }}
        className={cn(current && SELECTED)}
      >
        {props.allPlans ? null : <Dot dot={manager === undefined ? STOPPED : manager.dot} />}
        <span data-part="context-id" title={context.title} className="min-w-0 flex-1 truncate font-mono text-key font-semibold text-fg-strong">{context.id}</span>
      </SidebarMenuButton>
      <SidebarMenuAction aria-label={open ? "Collapse" : "Expand"} aria-expanded={open} onClick={() => setOpen(!open)}>
        <ChevronRightIcon size={14} aria-hidden="true" className={cn(open && "rotate-90")} />
      </SidebarMenuAction>
      {open ? (
        <SidebarMenuSub role="group">
          {plans.map((p) => (
            <PlanItem key={p.id} plan={p} {...props} />
          ))}
          {forks.map((s) => (
            <ForkRow key={s.ref.id} node={s} label={s.ref.key} {...props} />
          ))}
        </SidebarMenuSub>
      ) : null}
    </SidebarMenuItem>
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
  const [archivedOpen, setArchivedOpen] = useState(false);
  const live = tree === null ? [] : tree.contexts.filter((c) => !c.archived);
  const archived = tree === null ? [] : tree.contexts.filter((c) => c.archived);
  const archivedPlans = live.flatMap((c) => c.plans.filter((p) => p.archived).map((p) => ({ c, p })));
  const archivedCount = archived.length + archived.reduce((n, c) => n + c.plans.filter((p) => p.archived).length, 0) + archivedPlans.length;
  return (
    <Sidebar collapsible="none" aria-label="Context tree">
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel data-part="tree-header" className="text-[11px] font-semibold uppercase tracking-[.08em] text-muted-foreground">Contexts</SidebarGroupLabel>
          {onNewContext === undefined ? null : (
            <SidebarGroupAction data-action="new-context" aria-label="New context" onClick={onNewContext} className="text-fg-strong">
              <PlusIcon size={14} aria-hidden="true" />
            </SidebarGroupAction>
          )}
          <SidebarGroupContent>
            {tree === null ? (
              <p data-part="loading" className="px-2 text-fg-dim">Loading contexts…</p>
            ) : live.length === 0 && archived.length === 0 ? (
              <p data-part="empty" className="px-2 text-fg-dim">No contexts</p>
            ) : null}
            {tree === null ? null : (
              <SidebarMenu role="tree">
                {live.map((c) => <ContextItem key={c.id} context={c} allPlans={false} {...handlers} />)}
              </SidebarMenu>
            )}
          </SidebarGroupContent>
        </SidebarGroup>
        {archivedCount > 0 ? (
          <SidebarGroup data-archived>
            <SidebarGroupLabel className="p-0">
              <button
                type="button"
                aria-expanded={archivedOpen}
                onClick={() => setArchivedOpen(!archivedOpen)}
                className="flex h-8 w-full items-center px-2 text-left text-muted-foreground"
              >{`Archived (${archivedCount})`}</button>
            </SidebarGroupLabel>
            {archivedOpen ? (
              <SidebarGroupContent>
                <SidebarMenu role="tree">
                  {archived.map((c) => <ContextItem key={c.id} context={c} allPlans {...handlers} />)}
                  {archivedPlans.map(({ c, p }) => (
                    <SidebarMenuItem key={`${c.id}/${p.id}`}>
                      <SidebarMenuSub>
                        <PlanItem plan={p} {...handlers} />
                      </SidebarMenuSub>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            ) : null}
          </SidebarGroup>
        ) : null}
      </SidebarContent>
    </Sidebar>
  );
}
