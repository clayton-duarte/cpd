// The Context (left) rail (web/README.md "The Context tree in the left rail selects the Chat").
// The vendored shadcn `sidebar-03` block stops rendering the registry's sample rows: `groupSessions`
// groups the live Sessions by Context (each Context's manager is the root row, its leads nest
// beneath it, the Contexts sort by name) and `AppSidebar` renders that tree. Every row is a real
// <button> carrying its Session's id (`data-session-id` and `title`), the selected row carries
// `isActive`, and clicking a row reports it through `onSelectSession` (not `onSelect`: the DOM
// already declares that one on `Sidebar`).
//
// With the Contexts read wired the rail draws the cockpit's Context tree instead (web/README.md
// "Context tree rows"): every Context of the read, sorted by id with the archived ones last,
// each a Context row of glyph + id with no numbers and a fold button carrying `aria-expanded`. The
// fold, closed by default, hides only that Context's archived Plans while its live Plans always
// show; clicking a Context row selects its manager Session (disabled only when there is none) and
// clicking a Plan row selects its Lead Session. An awake row's dot takes its state's palette colour
// (working and waiting mizu, blocked anko, idle and unknown plain, asleep dim with its whole row
// dim), and no Tailwind hue is used anywhere. Given no Contexts read the rail keeps drawing its
// Session tree, so a Chat-only fake never regresses.
import * as React from "react"
import { GalleryVerticalEnd, ChevronRight } from "lucide-react"

import type { LiveContext, LivePlan, LivePlanCounts, LiveSession } from "../api/live-server"
import { cn } from "../lib/utils"
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "./ui/sidebar"

/** One Context: its manager (the root row) and its leads (the nested rows). */
export interface ContextGroup {
  context: string
  manager?: LiveSession
  leads: readonly LiveSession[]
}

/**
 * Groups Sessions by `context`: one group per Context, the manager at the root and the leads
 * nested; the groups sort by Context name. Pure — the component renders the result and nothing
 * else.
 */
export function groupSessions(sessions: readonly LiveSession[]): ContextGroup[] {
  const byContext = new Map<string, { manager?: LiveSession; leads: LiveSession[] }>()
  for (const session of sessions) {
    const context = session.context ?? ""
    let group = byContext.get(context)
    if (group === undefined) {
      group = { leads: [] }
      byContext.set(context, group)
    }
    if (session.role === "manager") {
      group.manager ??= session
    } else {
      group.leads.push(session)
    }
  }
  return [...byContext.entries()]
    .map(([context, group]) => ({ context, manager: group.manager, leads: group.leads }))
    .sort((a, b) => a.context.localeCompare(b.context))
}

/** One Context's Plans split by archived state, each half sorted by key. */
export interface ContextPlans {
  live: readonly LivePlan[]
  archived: readonly LivePlan[]
}

/**
 * The Plans of one Context, split into the live ones (always drawn) and the archived ones (behind
 * the Context row's fold), each sorted by Plan key (web/README.md "Context tree rows").
 */
export function plansForContext(plans: readonly LivePlan[], context: string): ContextPlans {
  const live: LivePlan[] = []
  const archived: LivePlan[] = []
  for (const plan of plans) {
    if (plan.context !== context) continue
    ;(plan.archived_at === null ? live : archived).push(plan)
  }
  live.sort((a, b) => a.id.localeCompare(b.id))
  archived.sort((a, b) => a.id.localeCompare(b.id))
  return { live, archived }
}

/** The Contexts in the rail's order: the live ones first, the archived after, each block sorted by id. */
export function orderContexts(contexts: readonly LiveContext[]): LiveContext[] {
  const byId = (a: LiveContext, b: LiveContext): number => a.id.localeCompare(b.id)
  return [
    ...contexts.filter((context) => !context.archived).sort(byId),
    ...contexts.filter((context) => context.archived).sort(byId),
  ]
}

/** A row's label: the Session's name, falling back to the given fallback. The id's first 8 characters show muted beside it. */
function RowLabel({ label, id }: { label: string; id: string }): React.ReactElement {
  return (
    <>
      <span>{label}</span>
      <span className="text-xs text-muted-foreground">{id.slice(0, 8)}</span>
    </>
  )
}

/**
 * A Session state's palette dot: the Dango custom property class plus whether the whole row is
 * muted (web/README.md "Context tree rows"). Working and waiting share the in-progress mizu;
 * blocked is anko; idle and an unknown state are the plain grey; asleep is the dim text. A
 * Session is asleep when the read's `asleep_at` is set, whatever its `state` string says; the
 * `"asleep"`/`"sleeping"` state literals stay as a fallback.
 */
function stateDot(
  state: string | undefined,
  asleepAt?: string | null,
): { colour: string; asleep: boolean } {
  if (asleepAt != null || state === "asleep" || state === "sleeping") {
    return { colour: "bg-[var(--dango-dim)]", asleep: true }
  }
  switch (state) {
    case "working":
    case "waiting":
      return { colour: "bg-[var(--dango-mizu)]", asleep: false }
    case "blocked":
      return { colour: "bg-[var(--dango-anko)]", asleep: false }
    case "idle":
    default:
      return { colour: "bg-[var(--dango-plain)]", asleep: false }
  }
}

/**
 * The dim grey with Tailwind's important modifier. The sidebar component's own
 * `text-sidebar-foreground` sits on the same element (web/src/components/ui/sidebar.tsx,
 * `sidebar-menu-sub-button`), and Tailwind's emitted order would let that win; the `!` makes the
 * dim grey win (web/README.md "Context tree rows").
 */
const DIM_TEXT = "text-[var(--dango-dim)]!"

/** The row's glyph: a Session state dot, no text, so a row's key is its only text. */
function Glyph({
  state,
  asleepAt,
}: {
  state: string | undefined
  asleepAt?: string | null
}): React.ReactElement {
  return (
    <span
      aria-hidden
      className={`size-2 shrink-0 rounded-full ${stateDot(state, asleepAt).colour}`}
    />
  )
}

/** One drawn Job counter: its number and the status colour it takes. */
interface DrawnCounter {
  value: number
  className: string
}

/**
 * The cockpit's four roles in its order (web/README.md "Context tree rows"): planned (draft +
 * queued) muted, active (running) the primary, failed the destructive and merged the chart-2
 * token. A zero is dropped, so a Plan with no counts draws nothing.
 */
export function planCounters(counts: LivePlanCounts): DrawnCounter[] {
  const drawn: DrawnCounter[] = [
    { value: counts.draft + counts.queued, className: "text-muted-foreground" },
    { value: counts.running, className: "text-primary" },
    { value: counts.failed, className: "text-destructive" },
    { value: counts.merged, className: "text-chart-2" },
  ]
  return drawn.filter((counter) => counter.value > 0)
}

/**
 * A Plan row's Job counters, a dot between two drawn roles, each number in its status colour.
 * An asleep row mutes the whole row, so every counter span — the `·` separator included — takes
 * the dim grey with the important modifier (web/README.md "Context tree rows").
 */
function PlanCounters({
  counts,
  asleep,
}: {
  counts: LivePlanCounts
  asleep: boolean
}): React.ReactElement | null {
  const drawn = planCounters(counts)
  if (drawn.length === 0) return null
  return (
    <span className="ml-auto flex shrink-0 items-center gap-1 text-xs tabular-nums">
      {drawn.map((counter, at) => (
        <React.Fragment key={counter.className}>
          {at > 0 ? <span className={asleep ? DIM_TEXT : "text-muted-foreground"}>·</span> : null}
          <span className={asleep ? DIM_TEXT : counter.className}>{counter.value}</span>
        </React.Fragment>
      ))}
    </span>
  )
}

/** One root row per Context (its manager); its leads nest beneath it in a `SidebarMenuSub`. */
function ContextRow({
  group,
  selectedId,
  onSelectSession,
}: {
  group: ContextGroup
  selectedId?: string
  onSelectSession: (id: string) => void
}): React.ReactElement {
  const { context, manager, leads } = group
  return (
    <SidebarMenuItem>
      {manager === undefined ? (
        // A Context with no manager still shows its root row, disabled.
        <SidebarMenuButton disabled>
          <RowLabel label={context} id="" />
        </SidebarMenuButton>
      ) : (
        <SidebarMenuButton
          isActive={selectedId === manager.id}
          data-session-id={manager.id}
          title={manager.id}
          onClick={() => onSelectSession(manager.id)}
        >
          <RowLabel label={manager.name ?? context} id={manager.id} />
        </SidebarMenuButton>
      )}
      {leads.length > 0 ? (
        <SidebarMenuSub>
          {leads.map((lead) => (
            <SidebarMenuSubItem key={lead.id}>
              <SidebarMenuSubButton asChild isActive={selectedId === lead.id}>
                <button
                  type="button"
                  data-session-id={lead.id}
                  title={lead.id}
                  onClick={() => onSelectSession(lead.id)}
                >
                  <RowLabel label={lead.name ?? lead.id} id={lead.id} />
                </button>
              </SidebarMenuSubButton>
            </SidebarMenuSubItem>
          ))}
        </SidebarMenuSub>
      ) : null}
    </SidebarMenuItem>
  )
}

/** The Session a Plan row's glyph reads: its Lead Session. */
function planSession(plan: LivePlan, sessions: readonly LiveSession[]): LiveSession | undefined {
  return plan.lead_session === null
    ? undefined
    : sessions.find((session) => session.id === plan.lead_session)
}

/**
 * One Plan row: glyph + key + its nonzero counters; clicking selects its Lead Session. The row's
 * button stretches to the rail's width (`w-full`), so the counters' `ml-auto` pushes them to the
 * pane's inner edge — the way the cockpit right-aligns a Plan's counters (web/README.md "Context
 * tree rows", hs-d841). Without the stretch a button shrink-wraps its content and the counters sit
 * inline right after the key.
 */
function PlanRow({
  plan,
  sessions,
  selectedId,
  onSelectSession,
}: {
  plan: LivePlan
  sessions: readonly LiveSession[]
  selectedId?: string
  onSelectSession: (id: string) => void
}): React.ReactElement {
  const lead = planSession(plan, sessions)
  const leadId = plan.lead_session
  // An asleep Lead's whole row (key and counters) is the palette's dim grey, its dot too; an
  // archived Plan's row is muted (web/README.md "Context tree rows").
  const asleep = stateDot(lead?.state, lead?.asleep_at).asleep
  const rowText = asleep
    ? DIM_TEXT
    : plan.archived_at !== null
      ? "text-muted-foreground"
      : undefined
  return (
    <SidebarMenuSubItem>
      <SidebarMenuSubButton asChild isActive={leadId !== null && selectedId === leadId}>
        <button
          type="button"
          title={leadId ?? plan.id}
          disabled={leadId === null}
          className={cn("w-full", rowText)}
          onClick={() => {
            if (leadId !== null) onSelectSession(leadId)
          }}
        >
          <Glyph state={lead?.state} asleepAt={lead?.asleep_at} />
          <span>{plan.id}</span>
          <PlanCounters counts={plan.counts} asleep={asleep} />
        </button>
      </SidebarMenuSubButton>
    </SidebarMenuSubItem>
  )
}

/**
 * One Context row: glyph + the Context's id, no numbers, and a fold button carrying `aria-expanded`
 * (web/README.md "Context tree rows"). Clicking the row selects the Context's manager Session and
 * is disabled only when there is none; the fold hides only that Context's archived Plans, folded by
 * default, while its live Plans always show. An archived Context is muted and starts folded.
 */
function ContextNode({
  context,
  plans,
  sessions,
  selectedId,
  onSelectSession,
}: {
  context: LiveContext
  plans: readonly LivePlan[]
  sessions: readonly LiveSession[]
  selectedId?: string
  onSelectSession: (id: string) => void
}): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false)
  const manager = sessions.find(
    (session) => session.context === context.id && session.role === "manager",
  )
  const { live, archived } = plansForContext(plans, context.id)
  const shown = expanded ? archived : []
  const muted = context.archived
  return (
    <SidebarMenuItem className={muted ? "text-muted-foreground" : undefined}>
      <SidebarMenuButton
        isActive={manager !== undefined && selectedId === manager.id}
        data-session-id={manager?.id}
        title={manager?.id}
        disabled={manager === undefined}
        onClick={() => {
          if (manager !== undefined) onSelectSession(manager.id)
        }}
      >
        <Glyph state={manager?.state} />
        <span>{context.id}</span>
      </SidebarMenuButton>
      <SidebarMenuAction
        aria-expanded={expanded}
        aria-label={expanded ? `collapse ${context.id}` : `expand ${context.id}`}
        onClick={() => setExpanded((open) => !open)}
      >
        <ChevronRight className={expanded ? "rotate-90" : undefined} />
      </SidebarMenuAction>
      {live.length + shown.length > 0 ? (
        <SidebarMenuSub>
          {[...live, ...shown].map((plan) => (
            <PlanRow
              key={plan.id}
              plan={plan}
              sessions={sessions}
              selectedId={selectedId}
              onSelectSession={onSelectSession}
            />
          ))}
        </SidebarMenuSub>
      ) : null}
    </SidebarMenuItem>
  )
}

/** The tree's props; `onSelectSession` is named so it does not collide with the DOM's `onSelect`. */
export interface AppSidebarProps {
  sessions: readonly LiveSession[]
  /** The Plans read; when absent, or empty, the rail draws its Session tree as before, and when present it draws the Plan tree (web/README.md "Context tree rows"). */
  plans?: readonly LivePlan[]
  /** The Contexts read; the corrected rail lists every Context from it (web/README.md "Context tree rows"). */
  contexts?: readonly LiveContext[]
  selectedId?: string
  onSelectSession: (id: string) => void
}

/** The Context tree: the left rail's only navigation. */
export function AppSidebar({
  sessions,
  plans,
  contexts,
  selectedId,
  onSelectSession,
  ...props
}: React.ComponentProps<typeof Sidebar> & AppSidebarProps): React.ReactElement {
  // Every Context of the Contexts read is the rail's row source (web/README.md "Context tree
  // rows"). With no Contexts read wired — or an empty one — the rail keeps the Session tree it
  // draws today, so a Chat-only fake never regresses ("The fallback").
  const ordered = contexts === undefined ? undefined : orderContexts(contexts)
  if (ordered !== undefined && ordered.length > 0) {
    const contextCount = ordered.length === 1 ? "1 context" : `${ordered.length} contexts`
    return (
      <Sidebar {...props}>
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton size="lg" asChild>
                <a href="#">
                  <div className="flex aspect-square size-8 items-center justify-center rounded-lg bg-sidebar-primary text-sidebar-primary-foreground">
                    <GalleryVerticalEnd className="size-4" />
                  </div>
                  <div className="flex flex-col gap-0.5 leading-none">
                    <span className="font-medium">cpd</span>
                    <span className="">{contextCount}</span>
                  </div>
                </a>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarMenu>
              {ordered.map((context) => (
                <ContextNode
                  key={context.id}
                  context={context}
                  plans={plans ?? []}
                  sessions={sessions}
                  selectedId={selectedId}
                  onSelectSession={onSelectSession}
                />
              ))}
            </SidebarMenu>
          </SidebarGroup>
        </SidebarContent>
      </Sidebar>
    )
  }

  const groups = groupSessions(sessions)
  const contextCount =
    groups.length === 0
      ? "No contexts"
      : `${groups.length} context${groups.length === 1 ? "" : "s"}`
  return (
    <Sidebar {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" asChild>
              <a href="#">
                <div className="flex aspect-square size-8 items-center justify-center rounded-lg bg-sidebar-primary text-sidebar-primary-foreground">
                  <GalleryVerticalEnd className="size-4" />
                </div>
                <div className="flex flex-col gap-0.5 leading-none">
                  <span className="font-medium">cpd</span>
                  <span className="">{contextCount}</span>
                </div>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarMenu>
            {groups.length === 0 ? (
              <SidebarMenuItem>
                <span className="px-2 py-1.5 text-sm text-sidebar-foreground/70">No contexts.</span>
              </SidebarMenuItem>
            ) : (
              groups.map((group) => (
                <ContextRow
                  key={group.context}
                  group={group}
                  selectedId={selectedId}
                  onSelectSession={onSelectSession}
                />
              ))
            )}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  )
}
