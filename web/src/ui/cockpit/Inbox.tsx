import {
  BellIcon,
  ChevronRightIcon,
  CircleAlertIcon,
  CircleDotIcon,
  CircleHelpIcon,
  ExternalLinkIcon,
  EyeIcon,
  GitPullRequestArrowIcon,
  GitPullRequestIcon,
  ListFilterIcon,
  MessageSquareIcon,
  RocketIcon,
  SparklesIcon,
} from "lucide-react";
import { useLayoutEffect, useRef, useState, type ReactElement } from "react";
import type { CockpitAction, InboxModel, InboxRow, SnoozeDuration, Urgency } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "../components/context-menu.tsx";
import { Item, ItemActions, ItemContent, ItemDescription, ItemGroup, ItemMedia, ItemTitle } from "../components/item.tsx";
import { Popover, PopoverContent, PopoverTrigger } from "../components/popover.tsx";
import { ToggleGroup, ToggleGroupItem } from "../components/toggle-group.tsx";
import { age, linkKinds, sessionKinds, urgencyText } from "./inbox/format.ts";

const SNOOZES: readonly [string, SnoozeDuration][] = [
  ["Snooze 15 min", "15m"],
  ["Snooze 1 h", "1h"],
  ["Snooze until tomorrow 09:00", "tomorrow"],
];

interface TierSpec {
  readonly urgency: Urgency;
  readonly label: string;
  readonly textColor: string;
}

const TIERS: readonly TierSpec[] = [
  {
    urgency: "red",
    label: "blocker",
    textColor:
      "text-status-blocked-bright hover:text-status-blocked-bright data-[state=on]:text-status-blocked-bright",
  },
  {
    urgency: "yellow",
    label: "delivery",
    textColor:
      "text-status-landing-bright hover:text-status-landing-bright data-[state=on]:text-status-landing-bright",
  },
  {
    urgency: "magenta",
    label: "decision",
    textColor:
      "text-status-decision-bright hover:text-status-decision-bright data-[state=on]:text-status-decision-bright",
  },
  {
    urgency: "blue",
    label: "support",
    textColor:
      "text-status-focus-bright hover:text-status-focus-bright data-[state=on]:text-status-focus-bright",
  },
  {
    urgency: "cyan",
    label: "notice",
    textColor:
      "text-status-active-bright hover:text-status-active-bright data-[state=on]:text-status-active-bright",
  },
  {
    urgency: "white",
    label: "unread",
    textColor: "text-fg-strong hover:text-fg-strong data-[state=on]:text-fg-strong",
  },
];

interface Handlers {
  onAction(action: CockpitAction): void;
  onSelectSession(sessionId: string): void;
}

function copy(text: string): void {
  void navigator.clipboard.writeText(text);
}

/** Left click, per kind (TUI inbox.rs jump). */
function jump(row: InboxRow, h: Handlers): void {
  if (row.kind === "engine-down") h.onAction({ kind: "engine.start" });
  else if (row.kind === "review-requested") {
    if (row.url !== null) copy(row.url);
  } else if (sessionKinds.includes(row.kind) && row.sessionId !== null) h.onSelectSession(row.sessionId);
}

function Menu(props: { row: InboxRow } & Handlers): ReactElement {
  const { row, onAction, onSelectSession } = props;
  const { id, planId, sessionId, url } = row;
  const workflow = row.workflow ?? null;
  const own: ReactElement[] = [];
  if (row.kind === "pr-ready" && planId !== null && workflow !== null) {
    own.push(
      <ContextMenuItem key="merge" onSelect={() => onAction({ kind: "pr.merge", planId, workflow })}>
        Merge
      </ContextMenuItem>,
    );
  }
  if (sessionId !== null && (row.kind === "pr-ready" || row.kind === "pr-closed")) {
    own.push(
      <ContextMenuItem key="lead" onSelect={() => onSelectSession(sessionId)}>
        Go to Lead
      </ContextMenuItem>,
    );
  }
  if (url !== null && (row.kind === "pr-ready" || row.kind === "pr-closed")) {
    own.push(
      <ContextMenuItem key="link" onSelect={() => copy(url)}>
        Copy Link
      </ContextMenuItem>,
    );
  }
  return (
    <ContextMenuContent>
      {own}
      {own.length > 0 ? <ContextMenuSeparator /> : null}
      {row.kind === "deploy" ? <ContextMenuItem onSelect={() => onAction({ kind: "inbox.approve", itemId: id })}>Approve</ContextMenuItem> : null}
      {row.kind === "review-requested" ? <ContextMenuItem onSelect={() => onAction({ kind: "inbox.claim", itemId: id })}>Claim</ContextMenuItem> : null}
      {SNOOZES.map(([label, duration]) => (
        <ContextMenuItem key={duration} onSelect={() => onAction({ kind: "inbox.snooze", itemId: id, duration })}>
          {label}
        </ContextMenuItem>
      ))}
      <ContextMenuItem onSelect={() => onAction({ kind: "inbox.dismiss", itemId: id })}>Dismiss</ContextMenuItem>
    </ContextMenuContent>
  );
}

function RowIcon(props: { kind: string; urgency: Urgency }): ReactElement {
  const { kind, urgency } = props;
  const cls = cn("size-4 shrink-0", urgencyText[urgency]);
  if (kind === "deploy") return <RocketIcon className={cls} />;
  if (kind === "pr-ready" || kind === "pr-closed") return <GitPullRequestIcon className={cls} />;
  if (kind === "review-requested") return <GitPullRequestArrowIcon className={cls} />;
  if (kind === "curate-retire") return <SparklesIcon className={cls} />;
  if (urgency === "red") return <CircleAlertIcon className={cls} />;
  if (urgency === "yellow") return <CircleDotIcon className={cls} />;
  if (urgency === "magenta") return <CircleHelpIcon className={cls} />;
  if (urgency === "blue") return <EyeIcon className={cls} />;
  if (urgency === "cyan") return <BellIcon className={cls} />;
  return <MessageSquareIcon className={cls} />;
}

function InboxItemRow(props: { row: InboxRow } & Handlers): ReactElement {
  const { row } = props;
  const [first = "", ...rest] = row.label.split("\n");
  const second = rest.join(" ").trim();
  const linked = row.url !== null && linkKinds.includes(row.kind);
  const item = (
    <Item variant="outline" size="sm" asChild data-kind={row.kind} data-urgency={row.urgency} data-id={row.id}>
      <a
        href={linked ? (row.url ?? "#") : "#"}
        target={linked ? "_blank" : undefined}
        rel={linked ? "noopener noreferrer" : undefined}
        onClick={(e) => {
          if (!linked) e.preventDefault();
          jump(row, props);
        }}
      >
        <ItemMedia>
          <RowIcon kind={row.kind} urgency={row.urgency} />
        </ItemMedia>
        <ItemContent>
          <ItemTitle className={urgencyText[row.urgency]}>{first}</ItemTitle>
          {second === "" ? null : <ItemDescription>{second}</ItemDescription>}
        </ItemContent>
        <ItemActions>
          <span data-part="age" className="font-mono text-xs text-muted-foreground">
            {age(row.ageSeconds)}
          </span>
          {linked ? <ExternalLinkIcon className="size-4 text-muted-foreground" /> : <ChevronRightIcon className="size-4 text-muted-foreground" />}
        </ItemActions>
      </a>
    </Item>
  );
  if (row.kind === "engine-down") return item;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{item}</ContextMenuTrigger>
      <Menu {...props} />
    </ContextMenu>
  );
}

export function Inbox(props: {
  inbox: InboxModel | null;
  onAction(action: CockpitAction): void;
  onSelectSession(sessionId: string): void;
}): ReactElement {
  const { inbox, onAction, onSelectSession } = props;
  const [selectedTiers, setSelectedTiers] = useState<"all" | ReadonlySet<Urgency>>("all");
  const [collapsed, setCollapsed] = useState(false);
  const headerRef = useRef<HTMLDivElement | null>(null);
  const measureRef = useRef<HTMLDivElement | null>(null);
  const shell = "relative flex h-full min-h-0 w-full flex-col overflow-hidden";
  const heading = "text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";
  const rows = inbox?.rows ?? [];
  const counts = new Map<Urgency, number>();
  for (const t of TIERS) counts.set(t.urgency, 0);
  for (const r of rows) counts.set(r.urgency, (counts.get(r.urgency) ?? 0) + 1);
  const activeValues: string[] = selectedTiers === "all" ? ["all", ...TIERS.map((t) => t.urgency)] : Array.from(selectedTiers);
  const toggleTier = (u: Urgency): void => {
    if (selectedTiers === "all") {
      setSelectedTiers(new Set<Urgency>([u]));
      return;
    }
    const next = new Set(selectedTiers);
    if (next.has(u)) next.delete(u);
    else next.add(u);
    setSelectedTiers(next.size === 0 || next.size === TIERS.length ? "all" : next);
  };
  const shown = selectedTiers === "all" ? rows : rows.filter((r) => selectedTiers.has(r.urgency));
  const countsKey = TIERS.map((t) => counts.get(t.urgency) ?? 0).join(",");
  useLayoutEffect(() => {
    const headerEl = headerRef.current;
    const measureEl = measureRef.current;
    if (headerEl === null || measureEl === null) return;
    const check = (): void => {
      const available = headerEl.clientWidth;
      const required = measureEl.scrollWidth;
      if (available > 0 && required > 0) setCollapsed(available < required);
    };
    check();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(check);
    ro.observe(headerEl);
    ro.observe(measureEl);
    return () => ro.disconnect();
  }, [countsKey, rows.length]);
  const renderFilterItems = (): ReactElement => (
    <>
      <ToggleGroupItem
        value="all"
        data-tier-filter="all"
        onClick={() => setSelectedTiers("all")}
        className="h-6 gap-1.5 px-1.5 text-xs text-fg-strong hover:text-fg-strong data-[state=on]:text-fg-strong"
      >
        <span>all</span>
        <span data-part="filter-count" className="font-mono text-[10px] opacity-80">
          {rows.length}
        </span>
      </ToggleGroupItem>
      {TIERS.map((t) => (
        <ToggleGroupItem
          key={t.urgency}
          value={t.urgency}
          data-tier-filter={t.label}
          data-urgency={t.urgency}
          onClick={() => toggleTier(t.urgency)}
          className={cn("h-6 gap-1.5 px-1.5 text-xs", t.textColor)}
        >
          <span>{t.label}</span>
          <span data-part="filter-count" className="font-mono text-[10px] opacity-80">
            {counts.get(t.urgency) ?? 0}
          </span>
        </ToggleGroupItem>
      ))}
    </>
  );
  const label = selectedTiers === "all" ? "all" : `${selectedTiers.size} tier${selectedTiers.size === 1 ? "" : "s"}`;
  const filterItem = "inline-flex h-6 items-center px-1.5 text-xs";
  return (
    <aside aria-label="Inbox" className={shell}>
      <div
        ref={measureRef}
        aria-hidden="true"
        className="pointer-events-none invisible absolute top-0 left-0 -z-10 flex h-8 w-max items-center gap-8 px-2.5"
      >
        <span className={cn(heading, "shrink-0")}>Inbox</span>
        <div className="flex shrink-0 items-center">
          <div className={filterItem}>
            <span>all</span>
            <span className="ml-1.5 font-mono text-[10px]">{rows.length}</span>
          </div>
          {TIERS.map((t) => (
            <div key={t.urgency} className={filterItem}>
              <span>{t.label}</span>
              <span className="ml-1.5 font-mono text-[10px]">{counts.get(t.urgency) ?? 0}</span>
            </div>
          ))}
        </div>
      </div>
      <div ref={headerRef} data-part="inbox-header" className="flex h-8 shrink-0 items-center justify-between gap-8 px-2.5">
        <h2 className={cn(heading, "shrink-0")}>Inbox</h2>
        {collapsed ? (
          <Popover>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="outline"
                size="sm"
                data-part="inbox-filters-trigger"
                aria-label="Filter inbox by tier"
                className="ml-auto h-6 shrink-0 gap-1.5 px-2 text-xs text-fg-strong shadow-none"
              >
                <ListFilterIcon className="size-3.5 text-muted-foreground" />
                <span>{label}</span>
                <span className="font-mono text-[10px] opacity-80">{shown.length}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-auto min-w-[8.5rem] p-1.5">
              <ToggleGroup
                type="multiple"
                variant="outline"
                size="sm"
                orientation="vertical"
                value={activeValues}
                aria-label="Filter inbox by tier"
                data-part="inbox-filters"
                className="w-full"
              >
                {renderFilterItems()}
              </ToggleGroup>
            </PopoverContent>
          </Popover>
        ) : (
          <ToggleGroup
            type="multiple"
            variant="outline"
            size="sm"
            orientation="horizontal"
            value={activeValues}
            aria-label="Filter inbox by tier"
            data-part="inbox-filters"
            className="ml-auto shrink-0"
          >
            {renderFilterItems()}
          </ToggleGroup>
        )}
      </div>
      {inbox === null ? (
        <p className="px-[14px] text-fg-dim">Loading inbox…</p>
      ) : shown.length === 0 ? (
        <p data-empty className="px-[14px] py-3 text-muted-foreground">
          {rows.length === 0 ? "up to date, drink some water" : "no items in this filter"}
        </p>
      ) : (
        <ItemGroup className="min-h-0 flex-1 gap-2 overflow-y-auto px-2.5 py-1.5">
          {shown.map((row) => (
            <InboxItemRow key={row.id} row={row} onAction={onAction} onSelectSession={onSelectSession} />
          ))}
        </ItemGroup>
      )}
    </aside>
  );
}
