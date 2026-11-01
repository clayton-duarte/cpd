// The Inbox rail: sidebar-09's mail list, vendored 1:1 from the `new-york-v4` registry into the
// shell's right column (web/README.md "The Inbox rail", "The live Inbox in the right rail"). Three
// parts of the block are deliberately not vendored: its first sidebar (the icon column,
// Inbox/Drafts/Sent/Junk/Trash, Acme Inc, the sample user), the `Unreads` switch that sat in this
// header, and the `Type to search...` input the block puts under the title. What is vendored is the
// block's second sidebar: its header (the title) and its mail list, rows and classes verbatim.
//
// The rows render the real Inbox read through the dev adapter, not registry sample data. The row's
// name is the item's actor, its date the item's age rendered relative, its subject the label's
// first line and its teaser the label's second line when there is one.
import type { ReactElement } from "react";
import type { LiveSession } from "../api/live-server";
import type { InboxItem } from "../api/readmodel";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
} from "./ui/sidebar";

/**
 * The row's name: the item's actor. A `review-requested` item is `peer review`; otherwise, when
 * the item names a Session, it is that Session's `name` (the first 8 characters of its id when the
 * name is absent); otherwise it is `cpd engine`.
 */
export function inboxActor(item: InboxItem, sessions: readonly LiveSession[]): string {
  if (item.kind === "review-requested") return "peer review";
  if (item.session !== null) {
    const name = sessions.find((session) => session.id === item.session)?.name;
    return name !== undefined && name !== "" ? name : item.session.slice(0, 8);
  }
  return "cpd engine";
}

/**
 * The row's date: one `age_seconds` rendered relative (web/README.md "The live Inbox in the right
 * rail"): `just now` under a minute, then minutes, hours, `Yesterday` from 24 to 48 hours, days
 * under a week, weeks under 30 days and months after that.
 */
export function relativeAge(ageSeconds: number): string {
  if (ageSeconds < 60) return "just now";
  if (ageSeconds < 3600) return `${Math.floor(ageSeconds / 60)} minutes ago`;
  if (ageSeconds < 86_400) return `${Math.floor(ageSeconds / 3600)} hours ago`;
  if (ageSeconds < 172_800) return "Yesterday";
  if (ageSeconds < 604_800) return `${Math.floor(ageSeconds / 86_400)} days ago`;
  if (ageSeconds < 2_592_000) return `${Math.floor(ageSeconds / 604_800)} weeks ago`;
  return `${Math.floor(ageSeconds / 2_592_000)} months ago`;
}

/** The subject line is the label's first line; the teaser is its second line when there is one. */
function labelParts(item: InboxItem): { subject: string; teaser?: string } {
  const lines = item.label.split("\n");
  const subject = lines[0] ?? "";
  const teaser = lines[1];
  return teaser === undefined ? { subject } : { subject, teaser };
}

/**
 * The Inbox rail: the block's mail list, one row per live item — the actor with its relative age
 * pushed right, the label's first line as the subject, then the second line as a two-line teaser
 * only when there is one — under its header's title. Each row is an anchor to the item's `url`, or
 * `#` when it has none; nothing in the rail writes.
 */
export function InboxRail({
  items,
  sessions,
}: {
  items: readonly InboxItem[];
  sessions: readonly LiveSession[];
}): ReactElement {
  return (
    <Sidebar collapsible="none">
      <SidebarHeader className="gap-3.5 border-b p-4">
        <div className="text-base font-medium text-foreground">Inbox</div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup className="px-0">
          <SidebarGroupContent>
            {items.map((item, index) => {
              const { subject, teaser } = labelParts(item);
              return (
                <a
                  href={item.url ?? "#"}
                  key={item.id ?? index}
                  className="flex flex-col items-start gap-2 border-b p-4 text-sm leading-tight whitespace-nowrap last:border-b-0 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                >
                  <div className="flex w-full items-center gap-2">
                    <span>{inboxActor(item, sessions)}</span>{" "}
                    <span className="ml-auto text-xs">{relativeAge(item.age_seconds)}</span>
                  </div>
                  <span className="font-medium">{subject}</span>
                  {teaser === undefined ? null : (
                    <span className="line-clamp-2 w-[260px] text-xs whitespace-break-spaces">
                      {teaser}
                    </span>
                  )}
                </a>
              );
            })}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  );
}
