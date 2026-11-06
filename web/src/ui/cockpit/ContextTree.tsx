import { CircleDashedIcon, CircleIcon, LoaderCircleIcon, MinusIcon, PlusIcon } from "lucide-react";
import { Fragment } from "react";
import type { ReactElement, ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import type { CockpitAction, ContextNode, ContextTreeModel, PlanCounts, PlanNode, SessionDot, SessionNode } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { formatCost } from "../../server/cockpit/cost.ts";
import { dotDisplay, sessionDisplay } from "../dango-status.ts";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "../components/context-menu.tsx";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../components/dialog.tsx";
import { Input } from "../components/input.tsx";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../components/collapsible.tsx";
import { ScrollArea } from "../components/scroll-area.tsx";
import { Separator } from "../components/separator.tsx";
import {
  Sidebar, SidebarContent, SidebarGroup, SidebarGroupAction, SidebarGroupContent, SidebarGroupLabel,
  SidebarMenu, SidebarMenuAction, SidebarMenuButton, SidebarMenuItem, SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem,
} from "../components/sidebar.tsx";
import { clearDraft } from "../composer/drafts.ts";
import { usePersistedBoolean } from "./persisted.ts";
import { PERMANENT_IDS, STOPPED, compareOrderKey, createdOf, isPermanent, leadOf, managerOf } from "../../state/archiveNav.ts";

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
  costsOn?: boolean;
  costs?: Costs;
  menu?: MenuApi;
}
type Costs = { readonly contexts: Readonly<Record<string, number>>; readonly plans: Readonly<Record<string, number>> };

// ---- context menus (pro-d46): items per row kind follow tui/src/tree/menu.rs ----
interface PromptSpec {
  readonly title: string;
  readonly prompt: string;
  readonly initial: string;
  /** A confirmation: the prompt is the text, there is no input and the button is `confirm`. */
  readonly confirm?: string;
  /** Returns the action to run, or the error to show in the dialog. */
  readonly submit: (value: string) => CockpitAction | string;
}
interface MenuApi {
  act(action: CockpitAction): void;
  ask(spec: PromptSpec): void;
  /** Every Plan id in every Context. */
  readonly taken: readonly string[];
}
interface MenuItem { readonly label: string; run(): void }
type MenuKind = "context" | "permanent" | "plan" | "fork" | "archived";

const PLAN_MAX = 24;
const NO_ID_ERROR = "A title needs at least two letters or digits, the first a letter, to make an id.";

/** tui/src/tree/slug.rs `derive`: lowercase ASCII slug, cut to `max`, `-2`, `-3`, ... while taken; null when no id fits. */
export function deriveSlug(title: string, max: number, taken: readonly string[]): string | null {
  let slug = "";
  let dash = false;
  for (const c of title) {
    if (/^[A-Za-z0-9]$/.test(c)) {
      if (slug === "" && !/^[A-Za-z]$/.test(c)) continue;
      if (dash && slug !== "") slug += "-";
      dash = false;
      slug += c.toLowerCase();
    } else dash = true;
  }
  const cut = (n: number): string => slug.slice(0, n).replace(/-+$/, "");
  const base = cut(max);
  if (base.length < 2) return null;
  if (!taken.includes(base)) return base;
  for (let n = 2; n < 10000; n++) {
    const id = `${cut(max - 1 - String(n).length)}-${n}`;
    if (!taken.includes(id)) return id;
  }
  return null;
}

const required = (what: "Title" | "Name", value: string): string | null => (value.trim() === "" ? `${what} is required.` : null);
const copyId = (id: string): void => { void navigator.clipboard.writeText(id); };

function renameItem(api: MenuApi, what: "Title" | "Name", initial: string, action: (v: string) => CockpitAction): MenuItem {
  return {
    label: "Rename\u2026",
    run: () => api.ask({ title: "Rename", prompt: `${what}:`, initial, submit: (v) => required(what, v) ?? action(v.trim()) }),
  };
}
const nameItem = (api: MenuApi, label: string, title: string, sourceId: string): MenuItem => ({
  label,
  run: () => api.ask({ title, prompt: "Name:", initial: "", submit: (v) => required("Name", v) ?? { kind: "session.fork", sourceId, name: v.trim() } }),
});
function newPlanItem(api: MenuApi, contextId: string): MenuItem {
  return {
    label: "New Plan\u2026",
    run: () => api.ask({
      title: "New Plan", prompt: "Title:", initial: "",
      submit: (v) => {
        const missing = required("Title", v);
        if (missing !== null) return missing;
        const planId = deriveSlug(v.trim(), PLAN_MAX, api.taken);
        return planId === null ? NO_ID_ERROR : { kind: "plan.new", planId, contextId, title: v.trim() };
      },
    }),
  };
}
const deleteItem = (api: MenuApi, id: string, action: CockpitAction): MenuItem => ({
  label: "Delete",
  run: () => api.ask({ title: "Delete", prompt: `Delete ${id}?`, initial: "", confirm: "Delete", submit: () => action }),
});
const copyItem = (id: string): MenuItem => ({ label: "Copy id", run: () => copyId(id) });
const actItem = (api: MenuApi, label: string, action: CockpitAction): MenuItem => ({ label, run: () => api.act(action) });

/** A Plan is quiet when archived or its Lead is stopped (the web's stand-in for the TUI's asleep). */
const isPlanQuiet = (p: PlanNode): boolean => {
  const lead = leadOf(p);
  return p.archived || lead === undefined || lead.dot.state === "stopped";
};

/** Wraps the row button in a right-click menu; without a menu API the row is plain. */
function RowMenu(props: { api: MenuApi | undefined; kind: MenuKind; items: readonly MenuItem[]; children: ReactElement }): ReactElement {
  if (props.api === undefined) return props.children;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{props.children}</ContextMenuTrigger>
      <ContextMenuContent data-menu-kind={props.kind}>
        {props.items.map((i) => (
          <ContextMenuItem key={i.label} onSelect={i.run}>{i.label}</ContextMenuItem>
        ))}
      </ContextMenuContent>
    </ContextMenu>
  );
}

function PromptDialog(props: { spec: PromptSpec | null; onDone(action: CockpitAction | null): void }): ReactElement {
  const { spec } = props;
  const [value, setValue] = useState(spec?.initial ?? "");
  const [error, setError] = useState<string | null>(null);
  const [lastSpec, setLastSpec] = useState(spec);
  if (spec !== lastSpec) { // a new prompt starts from its own initial value
    setLastSpec(spec);
    setValue(spec?.initial ?? "");
    setError(null);
  }
  const submit = (): void => {
    if (spec === null) return;
    const out = spec.submit(value);
    if (typeof out === "string") setError(out);
    else props.onDone(out);
  };
  return (
    <Dialog open={spec !== null} onOpenChange={(open) => { if (!open) props.onDone(null); }}>
      <DialogContent>
        <form onSubmit={(e) => { e.preventDefault(); submit(); }} className="grid gap-3">
          <DialogHeader>
            <DialogTitle>{spec?.title ?? ""}</DialogTitle>
            <DialogDescription>{spec?.prompt ?? ""}</DialogDescription>
          </DialogHeader>
          {spec?.confirm === undefined ? (
            <Input autoFocus aria-label={spec?.prompt ?? ""} aria-invalid={error !== null} value={value} onChange={(e) => { setValue(e.target.value); setError(null); }} />
          ) : null}
          {error === null ? null : <p role="alert" className="text-meta text-status-blocked-bright">{error}</p>}
          <DialogFooter>
            <button type="button" className="rounded-md border border-border px-3 py-1 text-sm" onClick={() => props.onDone(null)}>Cancel</button>
            <button type="button" className="rounded-md bg-primary px-3 py-1 text-sm text-primary-foreground" onClick={submit}>{spec?.confirm ?? "OK"}</button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The row's cost, drawn only while costs are on and the cost is known. */
function RowCost({ on, value }: { on: boolean | undefined; value: number | undefined }): ReactElement | null {
  if (on !== true || value === undefined) return null;
  return <span data-part="row-cost" className="shrink-0 font-mono text-meta text-fg-dim">{formatCost(value)}</span>;
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


function planWork(plan: PlanNode): "planning" | "dispatch" | "review" {
  if (plan.counts.draft > 0) return "planning";
  if ((plan.counts.merging ?? 0) > 0 || plan.phase === "deploy") return "review";
  if (plan.phase === "dispatch") return "dispatch";
  return "planning";
}

const ATTENTION_COLOUR: Record<string, string> = {
  red: "text-status-blocked-bright",
  yellow: "text-status-landing-bright",
  magenta: "text-status-decision-bright",
  cyan: "text-status-active-bright",
};
const WORK_COLOUR = {
  planning: "text-status-decision-bright",
  dispatch: "text-status-active-bright",
  review: "text-status-landing-bright",
} as const;

export function Dot({ dot, plan, archived = false }: { dot: SessionDot; plan?: PlanNode; archived?: boolean }): ReactElement {
  const { state } = dot;
  const attention = dot.attention === "blue" ? null : dot.attention;
  const label = attention === null ? state : `${state}, needs you (${attention})`;
  const d = sessionDisplay[dotDisplay(dot)];
  const base = "!size-3.5 shrink-0";
  const hooks = { role: "img", "data-dot": state, "data-attention": attention ?? "none", "data-tone": d.tone, "data-glyph": d.glyph, "aria-label": label } as const;

  if (archived) {
    const filled = state !== "working" && attention !== null;
    return <CircleIcon {...hooks} data-tone="neutral" data-attention={filled ? attention : "none"} className={cn(base, "text-fg-dim", filled && "fill-current")} />;
  }
  if (plan !== undefined && plan.counts.failed > 0 && state === "working") {
    return <LoaderCircleIcon {...hooks} data-dot="fixing" data-tone="blocked" data-glyph="active" aria-label="fixing failed job" className={cn(base, "animate-spin text-status-blocked-bright")} />;
  }
  if (state === "working") {
    const work = plan !== undefined ? planWork(plan) : null;
    return <LoaderCircleIcon {...hooks} data-work={work ?? "session"} data-attention="none" className={cn(base, "animate-spin", work === null ? "text-fg-strong" : WORK_COLOUR[work])} />;
  }
  if (attention !== null) {
    return <CircleIcon {...hooks} className={cn(base, "fill-current", ATTENTION_COLOUR[attention] ?? "text-fg-strong")} />;
  }
  if (state === "waiting") return <CircleDashedIcon {...hooks} className={cn(base, "text-status-active-bright")} />;
  if (state === "idle" || state === "blocked") return <CircleIcon {...hooks} className={cn(base, "text-fg-strong")} />;
  return <CircleIcon {...hooks} className={cn(base, "text-fg-dim")} />;
}

function ForkRow(props: { node: SessionNode; label: string } & Handlers): ReactElement {
  const { node } = props;
  const current = props.selectedSessionId === node.ref.id;
  const api = props.menu;
  const id = node.ref.id;
  const items: MenuItem[] = api === undefined ? [] : [
    { label: "Open", run: () => props.onSelectSession(id) },
    renameItem(api, "Name", props.label, (name) => ({ kind: "session.rename", sessionId: id, name })),
    nameItem(api, "Fork\u2026", "Fork", id),
    copyItem(id),
    // the selected Session may hold the chat slot, and the chat-slot Lead is never closed (commands.md 10.9)
    ...(current ? [] : [actItem(api, "Close", { kind: "session.close", sessionId: id })]),
    { label: "Archive", run: () => { clearDraft(id); api.act({ kind: "session.archive", sessionId: id }); } },
  ];
  return (
    <SidebarMenuSubItem data-part="session-row">
      <RowMenu api={api} kind="fork" items={items}>
      <SidebarMenuSubButton
        type="button"
        size="sm"
        isActive={current}
        data-session={node.ref.id}
        aria-current={current ? "true" : undefined}
        onClick={() => props.onSelectSession(node.ref.id)}
      >
        <Dot dot={node.dot} archived={node.archived} />
        <span className="min-w-0 flex-1 truncate font-mono">{props.label}</span>
      </SidebarMenuSubButton>
      </RowMenu>
    </SidebarMenuSubItem>
  );
}

const isFork = (s: SessionNode): boolean => s.ref.role === "lead" && !s.archived;

function PlanItem(props: { plan: PlanNode } & Handlers): ReactElement {
  const { plan } = props;
  const lead = leadOf(plan);
  const forks = plan.sessions.filter((s) => isFork(s) && s.ref.id !== plan.leadSessionId);
  const current = lead !== undefined && props.selectedSessionId === lead.ref.id;
  const api = props.menu;
  const rename = api === undefined ? [] : [renameItem(api, "Title", plan.title, (title) => ({ kind: "plan.rename", planId: plan.id, title }))];
  const del = api !== undefined && isPlanQuiet(plan) ? [deleteItem(api, plan.id, { kind: "plan.delete", planId: plan.id })] : [];
  const items: MenuItem[] = api === undefined ? [] : plan.archived
    ? [...rename, copyItem(plan.id), actItem(api, "Unarchive", { kind: "plan.unarchive", planId: plan.id }), ...del]
    : [
        ...(lead === undefined ? [] : [nameItem(api, "New session\u2026", "New session", lead.ref.id)]),
        ...rename,
        copyItem(plan.id),
        actItem(api, "Archive", { kind: "plan.archive", planId: plan.id }),
        ...del,
      ];
  return (
    <SidebarMenuSubItem role="treeitem" data-plan={plan.id} data-part="plan-row">
      <RowMenu api={api} kind={plan.archived ? "archived" : "plan"} items={items}>
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
        <Dot dot={lead === undefined ? STOPPED : lead.dot} plan={plan} archived={plan.archived} />
        <span data-part="plan-id" title={plan.title} className="min-w-0 flex-1 truncate font-mono text-key">{plan.id}</span>
        <RowCost on={props.costsOn} value={props.costs?.plans[plan.id]} />
        <Counters counts={plan.counts} />
      </SidebarMenuSubButton>
      </RowMenu>
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
  // pro-d78: a selected Manager collapses its Context row transiently (never persisted)
  const [overrideWhileCurrent, setOverrideWhileCurrent] = useState<boolean | null>(null);
  const prevCurrent = useRef(current);
  useEffect(() => {
    if (prevCurrent.current === current) return;
    prevCurrent.current = current;
    setOverrideWhileCurrent(null);
    if (!current && !props.allPlans && !open) setOpen(true);
  }, [current, open, props.allPlans, setOpen]);
  const effectiveOpen = !props.allPlans && current ? (overrideWhileCurrent ?? false) : open;
  const handleOpenChange = (next: boolean): void => {
    if (!props.allPlans && current) setOverrideWhileCurrent(next);
    else setOpen(next);
  };
  const api = props.menu;
  const permanent = isPermanent(context.id);
  const archivedRow = props.allPlans && context.archived;
  const deletable = !permanent && (context.archived || (context.plans.length > 0 && context.plans.every(isPlanQuiet)));
  const del = api !== undefined && deletable ? [deleteItem(api, context.id, { kind: "context.delete", contextId: context.id })] : [];
  const rename = api === undefined ? [] : [renameItem(api, "Title", context.title, (title) => ({ kind: "context.rename", contextId: context.id, title }))];
  const items: MenuItem[] = api === undefined ? [] : permanent
    ? [newPlanItem(api, context.id), copyItem(context.id)]
    : archivedRow
      ? [...rename, copyItem(context.id), actItem(api, "Unarchive", { kind: "context.unarchive", contextId: context.id }), ...del]
      : [newPlanItem(api, context.id), ...rename, copyItem(context.id), actItem(api, "Archive", { kind: "context.archive", contextId: context.id }), ...del];
  // in the Archive section a live Context only holds archived Plans: its own row has no menu
  const rowApi = props.allPlans && !context.archived ? undefined : api;
  return (
    <Collapsible open={effectiveOpen} onOpenChange={handleOpenChange} asChild>
      <SidebarMenuItem role="treeitem" data-context={context.id}>
        <RowMenu api={rowApi} kind={permanent ? "permanent" : archivedRow ? "archived" : "context"} items={items}>
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
          <Dot dot={manager === undefined ? STOPPED : manager.dot} archived={props.allPlans || context.archived} />
          <span data-part="context-id" title={context.title} className="min-w-0 flex-1 truncate font-mono text-key font-semibold text-fg-strong">{context.id}</span>
          <RowCost on={props.costsOn} value={props.costs?.contexts[context.id]} />
        </SidebarMenuButton>
        </RowMenu>
        {hasChildren ? (
          <>
            <CollapsibleTrigger asChild>
              <SidebarMenuAction type="button" aria-label={`Toggle ${context.id}`} data-state={effectiveOpen ? "open" : "closed"}>
                {effectiveOpen ? <MinusIcon /> : <PlusIcon />}
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
  section: "contexts" | "standing" | "archive";
  label: string;
  open: boolean;
  onOpenChange(open: boolean): void;
  action?: ReactNode;
  children: ReactNode;
  headerDataPart?: string;
  groupAttrs?: Record<string, string | boolean>;
}): ReactElement {
  return (
    <Collapsible
      open={props.open}
      onOpenChange={props.onOpenChange}
      data-part="tree-section"
      data-section={props.section}
      className={cn("group/collapsible flex flex-col overflow-hidden", props.open ? "min-h-0 flex-1" : "shrink-0")}
    >
      <SidebarGroup className={cn("flex flex-col overflow-hidden px-0 py-1", props.open ? "min-h-0 flex-1" : "shrink-0")} {...props.groupAttrs}>
        <div className="relative shrink-0 px-2">
          <SidebarGroupLabel asChild data-part={props.headerDataPart}>
            <CollapsibleTrigger
              type="button"
              className={cn(
                "w-full hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
                props.action !== undefined && "pr-7",
              )}
            >
              <span>{props.label}</span>
              {props.open ? <MinusIcon className="ml-auto" /> : <PlusIcon className="ml-auto" />}
            </CollapsibleTrigger>
          </SidebarGroupLabel>
          {props.action}
        </div>
        <CollapsibleContent className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <ScrollArea data-part="tree-scroll" className="min-h-0 flex-1 [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:w-full [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:!min-w-0">
            <SidebarGroupContent className="px-2">{props.children}</SidebarGroupContent>
          </ScrollArea>
        </CollapsibleContent>
      </SidebarGroup>
    </Collapsible>
  );
}

function ContextList(props: { contexts: ContextNode[]; allPlans: boolean } & Handlers): ReactElement {
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
  /** Costs by Context id and Plan id; drawn only while `costsOn`. */
  costsOn?: boolean;
  costs?: Costs;
  /** Runs a tree menu action; without it the rows have no menu. */
  onAction?: (action: CockpitAction) => void;
}): ReactElement {
  const { tree, onNewContext, onAction, ...rest } = props;
  const [prompt, setPrompt] = useState<PromptSpec | null>(null);
  const taken = (tree?.contexts ?? []).flatMap((c) => c.plans.map((p) => p.id));
  const menu: MenuApi | undefined = onAction === undefined ? undefined : { act: onAction, ask: setPrompt, taken };
  const handlers = menu === undefined ? rest : { ...rest, menu };
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

  const showStanding = standing.length > 0 || archived.length > 0;

  return (
    <Sidebar collapsible="none" aria-label="Context tree" className="h-full overflow-hidden bg-background">
      <SidebarContent className="flex h-full min-h-0 flex-col gap-0 overflow-hidden">
        <TreeSection
          section="contexts"
          label="Contexts"
          open={contextsOpen}
          onOpenChange={setContextsOpen}
          headerDataPart="tree-header"
          action={
            onNewContext === undefined ? undefined : (
              <SidebarGroupAction type="button" data-action="new-context" aria-label="New context" onClick={onNewContext} className="top-1 right-3 z-10">
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
            <ContextList contexts={projectContexts} allPlans={false} {...handlers} />
          )}
        </TreeSection>
        {showStanding ? <Separator className="shrink-0" /> : null}
        {showStanding ? (
          <TreeSection section="standing" label="Standing" open={standingOpen} onOpenChange={setStandingOpen} groupAttrs={{ "data-standing": true }}>
            <ContextList contexts={standing} allPlans={false} {...handlers} />
          </TreeSection>
        ) : null}
        {archived.length > 0 ? <Separator className="shrink-0" /> : null}
        {archived.length > 0 ? (
          <TreeSection section="archive" label="Archive" open={archivedOpen} onOpenChange={setArchivedOpen} groupAttrs={{ "data-archived": true }}>
            <ContextList contexts={archived} allPlans={true} {...handlers} />
          </TreeSection>
        ) : null}
      </SidebarContent>
      <PromptDialog spec={prompt} onDone={(action) => { setPrompt(null); if (action !== null) onAction?.(action); }} />
    </Sidebar>
  );
}
