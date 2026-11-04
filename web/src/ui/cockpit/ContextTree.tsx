import { MinusIcon, PlusIcon } from "lucide-react";
import { Fragment } from "react";
import type { ReactElement, ReactNode } from "react";
import type { ContextNode, ContextTreeModel, PlanCounts, PlanNode, SessionDot, SessionNode } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { dotDisplay, glyphClass, sessionDisplay } from "../dango-status.ts";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../components/collapsible.tsx";
import { ScrollArea } from "../components/scroll-area.tsx";
import { Separator } from "../components/separator.tsx";
import {
  Sidebar, SidebarContent, SidebarGroup, SidebarGroupAction, SidebarGroupContent, SidebarGroupLabel,
  SidebarMenu, SidebarMenuAction, SidebarMenuButton, SidebarMenuItem, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem,
} from "../components/sidebar.tsx";
import { usePersistedBoolean } from "./persisted.ts";

/** Standing Contexts in TUI order (PermanentContext::ALL), never sorted. */
const PERMANENT_IDS = ["cpd", "host", "codereview"];
const isPermanent = (id: string): boolean => PERMANENT_IDS.includes(id);
const createdOf = (n: object): string => (n as { createdAt?: string }).createdAt ?? "";

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
    <span data-part="counters" className="ml-auto shrink-0 font-mono text-meta">
      {shown.map(([k, tone], i) => (
        <Fragment key={k}>
          {i > 0 ? " " : null}
          <span data-count={k} className={tone}>{String(counts[k])}</span>
        </Fragment>
      ))}
    </span>
  );
}

const STOPPED: SessionDot = { state: "stopped", attention: null };

function Dot({ dot }: { dot: SessionDot }): ReactElement {
  const { state, attention } = dot;
  const label = attention === null ? state : `${state}, needs you (${attention})`;
  const d = sessionDisplay[dotDisplay(dot)];
  return (
    <span role="img" data-dot={state} data-attention={attention ?? "none"} data-tone={d.tone} data-glyph={d.glyph} aria-label={label} className={glyphClass[d.glyph]} />
  );
}

/** TUI Dot::tier: red 0, yellow 1, magenta 2, cyan 3, everything else (and working) 4. */
function dotTier(dot: SessionDot): number {
  if (dot.state === "working") return 4;
  switch (dot.attention) {
    case "red": return 0;
    case "yellow": return 1;
    case "magenta": return 2;
    case "cyan": return 3;
    default: return 4;
  }
}

/** (tier, newest first); the stable sort keeps input order on ties. */
function compareOrderKey(aDot: SessionDot, aCreated: string, bDot: SessionDot, bCreated: string): number {
  const tierDiff = dotTier(aDot) - dotTier(bDot);
  if (tierDiff !== 0) return tierDiff;
  if (aCreated > bCreated) return -1;
  if (aCreated < bCreated) return 1;
  return 0;
}

function ForkRow(props: { node: SessionNode; label: string } & Handlers): ReactElement {
  const { node } = props;
  const current = props.selectedSessionId === node.ref.id;
  return (
    <SidebarMenuSubItem data-part="session-row">
      <SidebarMenuSubButton
        type="button"
        size="sm"
        isActive={current}
        data-session={node.ref.id}
        aria-current={current ? "true" : undefined}
        onClick={() => props.onSelectSession(node.ref.id)}
      >
        <Dot dot={node.dot} />
        <span className="min-w-0 flex-1 truncate font-mono">{props.label}</span>
      </SidebarMenuSubButton>
    </SidebarMenuSubItem>
  );
}

const isFork = (s: SessionNode): boolean => s.ref.role === "lead" && !s.archived;
const leadOf = (p: PlanNode): SessionNode | undefined => p.sessions.find((s) => s.ref.id === p.leadSessionId);
const managerOf = (c: ContextNode): SessionNode | undefined =>
  c.sessions.find((s) => s.ref.role === "manager" && !s.archived);

function PlanItem(props: { plan: PlanNode } & Handlers): ReactElement {
  const { plan } = props;
  const lead = leadOf(plan);
  const forks = plan.sessions.filter((s) => isFork(s) && s.ref.id !== plan.leadSessionId);
  const current = lead !== undefined && props.selectedSessionId === lead.ref.id;
  return (
    <SidebarMenuSubItem role="treeitem" data-plan={plan.id} data-part="plan-row">
      <SidebarMenuSubButton
        type="button"
        isActive={current}
        data-select="plan"
        aria-current={current ? "true" : undefined}
        onClick={() => {
          props.onSelectPlan(plan.id);
          if (lead !== undefined) props.onSelectSession(lead.ref.id);
        }}
      >
        <Dot dot={lead === undefined ? STOPPED : lead.dot} />
        <span data-part="plan-id" title={plan.title} className="min-w-0 flex-1 truncate font-mono text-key">{plan.id}</span>
        <Counters counts={plan.counts} />
      </SidebarMenuSubButton>
      {forks.length > 0 ? (
        <SidebarMenuSub>
          {forks.map((s) => (
            <ForkRow key={s.ref.id} node={s} label={`${plan.id}·${s.ref.id.slice(0, 4)}`} {...props} />
          ))}
        </SidebarMenuSub>
      ) : null}
    </SidebarMenuSubItem>
  );
}

function ContextItem(props: { context: ContextNode; allPlans: boolean } & Handlers): ReactElement {
  const { context } = props;
  const [open, setOpen] = usePersistedBoolean(`ctx:${props.allPlans ? "arch:" : ""}${context.id}`, true);
  const plans = (props.allPlans ? context.plans : context.plans.filter((p) => !p.archived))
    .slice()
    .sort((a, b) => compareOrderKey(leadOf(a)?.dot ?? STOPPED, createdOf(a), leadOf(b)?.dot ?? STOPPED, createdOf(b)));
  const manager = managerOf(context);
  const forks = context.sessions.filter(isFork);
  const hasChildren = plans.length > 0 || forks.length > 0;
  const current = manager !== undefined && props.selectedSessionId === manager.ref.id;
  return (
    <Collapsible open={open} onOpenChange={setOpen} asChild>
      <SidebarMenuItem role="treeitem" data-context={context.id}>
        <SidebarMenuButton
          type="button"
          isActive={current}
          data-select="context"
          aria-current={current ? "true" : undefined}
          onClick={() => {
            if (manager !== undefined) props.onSelectSession(manager.ref.id);
          }}
          className={hasChildren ? "pr-7" : undefined}
        >
          {props.allPlans ? null : <Dot dot={manager === undefined ? STOPPED : manager.dot} />}
          <span data-part="context-id" title={context.title} className="min-w-0 flex-1 truncate font-mono text-key font-semibold text-fg-strong">{context.id}</span>
        </SidebarMenuButton>
        {hasChildren ? (
          <>
            <CollapsibleTrigger asChild>
              <SidebarMenuAction type="button" aria-label={`Toggle ${context.id}`} data-state={open ? "open" : "closed"}>
                {open ? <MinusIcon /> : <PlusIcon />}
              </SidebarMenuAction>
            </CollapsibleTrigger>
            <CollapsibleContent>
              <SidebarMenuSub role="group">
                {plans.map((p) => (
                  <PlanItem key={p.id} plan={p} {...props} />
                ))}
                {forks.map((s) => (
                  <ForkRow key={s.ref.id} node={s} label={s.ref.key} {...props} />
                ))}
              </SidebarMenuSub>
            </CollapsibleContent>
          </>
        ) : null}
      </SidebarMenuItem>
    </Collapsible>
  );
}

function TreeSection(props: {
  label: string;
  open: boolean;
  onOpenChange(open: boolean): void;
  action?: ReactNode;
  children: ReactNode;
  headerDataPart?: string;
  groupAttrs?: Record<string, string | boolean>;
}): ReactElement {
  return (
    <Collapsible open={props.open} onOpenChange={props.onOpenChange} className="group/collapsible">
      <SidebarGroup className="px-2 py-0" {...props.groupAttrs}>
        <SidebarGroupLabel data-part={props.headerDataPart} className="p-0">
          <CollapsibleTrigger
            type="button"
            className={cn(
              "flex h-full w-full items-center rounded-md px-2 text-left hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
              props.action !== undefined && "pr-7",
            )}
          >
            <span>{props.label}</span>
            {props.open ? <MinusIcon className="ml-auto" /> : <PlusIcon className="ml-auto" />}
          </CollapsibleTrigger>
        </SidebarGroupLabel>
        {props.action}
        <CollapsibleContent>
          <SidebarGroupContent>{props.children}</SidebarGroupContent>
        </CollapsibleContent>
      </SidebarGroup>
    </Collapsible>
  );
}

function ContextMenu(props: { contexts: ContextNode[]; allPlans: boolean } & Handlers): ReactElement {
  const { contexts, allPlans, ...handlers } = props;
  return (
    <SidebarMenu role="tree">
      {contexts.map((c) => (
        <ContextItem key={`${allPlans ? "arch-" : ""}${c.id}`} context={c} allPlans={allPlans} {...handlers} />
      ))}
    </SidebarMenu>
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
  const [contextsOpen, setContextsOpen] = usePersistedBoolean("section:contexts", true);
  const [standingOpen, setStandingOpen] = usePersistedBoolean("section:standing", true);
  const [archivedOpen, setArchivedOpen] = usePersistedBoolean("section:archive", false);

  const all = tree === null ? [] : tree.contexts;
  const projectContexts = all
    .filter((c) => !c.archived && !isPermanent(c.id))
    .sort((a, b) => compareOrderKey(managerOf(a)?.dot ?? STOPPED, createdOf(a), managerOf(b)?.dot ?? STOPPED, createdOf(b)));
  const standing = PERMANENT_IDS.map((id) => all.find((c) => c.id === id)).filter(
    (c): c is ContextNode => c !== undefined,
  );
  const archived: ContextNode[] = [
    ...all.filter((c) => c.archived && !isPermanent(c.id)),
    ...all
      .filter((c) => !c.archived && c.plans.some((p) => p.archived))
      .map((c) => ({ ...c, plans: c.plans.filter((p) => p.archived) })),
  ];

  return (
    <Sidebar collapsible="none" aria-label="Context tree" className="h-full overflow-hidden bg-background">
      <SidebarContent className="flex h-full min-h-0 flex-col gap-0 overflow-hidden">
        <ScrollArea className="min-h-0 flex-1 py-1">
          <TreeSection
            label="Contexts"
            open={contextsOpen}
            onOpenChange={setContextsOpen}
            headerDataPart="tree-header"
            action={
              onNewContext === undefined ? undefined : (
                <SidebarGroupAction type="button" data-action="new-context" aria-label="New context" onClick={onNewContext} className="top-1 right-3">
                  <PlusIcon size={14} aria-hidden="true" />
                </SidebarGroupAction>
              )
            }
          >
            {tree === null ? (
              <p data-part="loading" className="px-2 text-fg-dim">Loading contexts…</p>
            ) : all.length === 0 ? (
              <p data-part="empty" className="px-2 text-fg-dim">No contexts</p>
            ) : (
              <ContextMenu contexts={projectContexts} allPlans={false} {...handlers} />
            )}
          </TreeSection>
        </ScrollArea>

        {standing.length > 0 || archived.length > 0 ? (
          <div data-part="tree-pinned-bottom" className="mt-auto flex max-h-[50%] shrink-0 flex-col overflow-hidden">
            <Separator />
            <ScrollArea className="min-h-0 flex-1">
              <div className="flex flex-col gap-1 py-1">
                <TreeSection label="Standing" open={standingOpen} onOpenChange={setStandingOpen} groupAttrs={{ "data-standing": true }}>
                  <ContextMenu contexts={standing} allPlans={false} {...handlers} />
                </TreeSection>
                {archived.length > 0 ? <Separator /> : null}
                {archived.length > 0 ? (
                  <TreeSection label="Archive" open={archivedOpen} onOpenChange={setArchivedOpen} groupAttrs={{ "data-archived": true }}>
                    <ContextMenu contexts={archived} allPlans={true} {...handlers} />
                  </TreeSection>
                ) : null}
              </div>
            </ScrollArea>
          </div>
        ) : null}
      </SidebarContent>
    </Sidebar>
  );
}
