// The Context (left) rail (web/README.md "The Context tree in the left rail selects the Chat").
// The vendored shadcn `sidebar-03` block stops rendering the registry's sample rows: `groupSessions`
// groups the live Sessions by Context (each Context's manager is the root row, its leads nest
// beneath it, the Contexts sort by name) and `AppSidebar` renders that tree. Every row is a real
// <button> carrying its Session's id (`data-session-id` and `title`), the selected row carries
// `isActive`, and clicking a row reports it through `onSelectSession` (not `onSelect`: the DOM
// already declares that one on `Sidebar`).
//
// With the Plans read wired it renders the Plan tree instead (web/README.md "Context tree rows"): a
// Context row of glyph + key with no numbers, then one Plan row per non-archived Plan of that
// Context, sorted by Context name and then by Plan key. A Plan row is glyph + key + its nonzero Job
// counters in the cockpit's order (planned, active, failed, merged), a dot between two drawn roles,
// each in its status colour; clicking a Plan row selects its Lead Session. Given no plans at all the
// rail keeps drawing its Session tree, so a Chat-only fake never regresses.
import * as React from "react"
import { GalleryVerticalEnd } from "lucide-react"

import type { LiveContext, LivePlan, LivePlanCounts, LiveSession } from "../api/live-server"
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarHeader,
  SidebarMenu,
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

/** One Context's Plans, the Context name and its Plans sorted by key. */
interface PlanGroup {
  context: string
  plans: readonly LivePlan[]
}

/** Groups Plans by Context, sorting the Contexts by name and each Context's Plans by key. */
export function groupPlans(plans: readonly LivePlan[]): PlanGroup[] {
  const byContext = new Map<string, LivePlan[]>()
  for (const plan of plans) {
    const group = byContext.get(plan.context)
    if (group === undefined) byContext.set(plan.context, [plan])
    else group.push(plan)
  }
  return [...byContext.entries()]
    .map(([context, contextPlans]) => ({
      context,
      plans: [...contextPlans].sort((a, b) => a.id.localeCompare(b.id)),
    }))
    .sort((a, b) => a.context.localeCompare(b.context))
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

/** The state dot colour for a Session's state (web/README.md "Context tree rows"). */
function stateDot(state: string | undefined): string {
  switch (state) {
    case "working":
      return "bg-emerald-500"
    case "waiting":
      return "bg-violet-500"
    case "blocked":
      return "bg-red-500"
    case "idle":
      return "bg-muted-foreground"
    default:
      return "bg-muted-foreground"
  }
}

/** The row's glyph: a Session state dot, no text, so a row's key is its only text. */
function Glyph({ state }: { state: string | undefined }): React.ReactElement {
  return <span aria-hidden className={`size-2 shrink-0 rounded-full ${stateDot(state)}`} />
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

/** A Plan row's Job counters, a dot between two drawn roles, each number in its status colour. */
function PlanCounters({ counts }: { counts: LivePlanCounts }): React.ReactElement | null {
  const drawn = planCounters(counts)
  if (drawn.length === 0) return null
  return (
    <span className="ml-auto flex shrink-0 items-center gap-1 text-xs tabular-nums">
      {drawn.map((counter, at) => (
        <React.Fragment key={counter.className}>
          {at > 0 ? <span className="text-muted-foreground">·</span> : null}
          <span className={counter.className}>{counter.value}</span>
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

/** One Plan row: glyph + key + its nonzero counters; clicking selects its Lead Session. */
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
  return (
    <SidebarMenuSubItem>
      <SidebarMenuSubButton asChild isActive={leadId !== null && selectedId === leadId}>
        <button
          type="button"
          title={leadId ?? plan.id}
          disabled={leadId === null}
          onClick={() => {
            if (leadId !== null) onSelectSession(leadId)
          }}
        >
          <Glyph state={lead?.state} />
          <span>{plan.id}</span>
          <PlanCounters counts={plan.counts} />
        </button>
      </SidebarMenuSubButton>
    </SidebarMenuSubItem>
  )
}

/** One Context row (glyph + key, no numbers) with its Plan rows nested beneath it. */
function PlanContextRow({
  group,
  sessions,
  selectedId,
  onSelectSession,
}: {
  group: PlanGroup
  sessions: readonly LiveSession[]
  selectedId?: string
  onSelectSession: (id: string) => void
}): React.ReactElement {
  const manager = sessions.find(
    (session) => session.context === group.context && session.role === "manager",
  )
  return (
    <SidebarMenuItem>
      <SidebarMenuButton disabled>
        <Glyph state={manager?.state} />
        <span>{group.context}</span>
      </SidebarMenuButton>
      {group.plans.length > 0 ? (
        <SidebarMenuSub>
          {group.plans.map((plan) => (
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
  contexts: _contexts,
  selectedId,
  onSelectSession,
  ...props
}: React.ComponentProps<typeof Sidebar> & AppSidebarProps): React.ReactElement {
  // With the Plans read wired and plans to draw, the rail is the Plan tree; otherwise it keeps the
  // Session tree it draws today (web/README.md "The fallback").
  const planGroups = plans === undefined || plans.length === 0 ? undefined : groupPlans(plans)
  if (planGroups !== undefined) {
    const contextCount =
      planGroups.length === 0
        ? "No contexts"
        : `${planGroups.length} context${planGroups.length === 1 ? "" : "s"}`
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
              {planGroups.map((group) => (
                <PlanContextRow
                  key={group.context}
                  group={group}
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

  const contexts = groupSessions(sessions)
  const contextCount =
    contexts.length === 0
      ? "No contexts"
      : `${contexts.length} context${contexts.length === 1 ? "" : "s"}`
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
            {contexts.length === 0 ? (
              <SidebarMenuItem>
                <span className="px-2 py-1.5 text-sm text-sidebar-foreground/70">No contexts.</span>
              </SidebarMenuItem>
            ) : (
              contexts.map((group) => (
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
