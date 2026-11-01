// The Context (left) rail (web/README.md "The Context tree in the left rail selects the Chat").
// The vendored shadcn `sidebar-03` block stops rendering the registry's sample rows: `groupSessions`
// groups the live Sessions by Context (each Context's manager is the root row, its leads nest
// beneath it, the Contexts sort by name) and `AppSidebar` renders that tree. Every row is a real
// <button> carrying its Session's id (`data-session-id` and `title`), the selected row carries
// `isActive`, and clicking a row reports it through `onSelectSession` (not `onSelect`: the DOM
// already declares that one on `Sidebar`).
import * as React from "react"
import { GalleryVerticalEnd } from "lucide-react"

import type { LiveSession } from "../api/live-server"
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

/** A row's label: the Session's name, falling back to the given fallback. The id's first 8 characters show muted beside it. */
function RowLabel({ label, id }: { label: string; id: string }): React.ReactElement {
  return (
    <>
      <span>{label}</span>
      <span className="text-xs text-muted-foreground">{id.slice(0, 8)}</span>
    </>
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

/** The tree's props; `onSelectSession` is named so it does not collide with the DOM's `onSelect`. */
export interface AppSidebarProps {
  sessions: readonly LiveSession[]
  selectedId?: string
  onSelectSession: (id: string) => void
}

/** The Context tree: the left rail's only navigation. */
export function AppSidebar({
  sessions,
  selectedId,
  onSelectSession,
  ...props
}: React.ComponentProps<typeof Sidebar> & AppSidebarProps): React.ReactElement {
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
