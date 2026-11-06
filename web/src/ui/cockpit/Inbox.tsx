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
  LoaderCircleIcon,
  MessageSquareIcon,
  RocketIcon,
  SparklesIcon,
} from "lucide-react";
import {
  type ReactElement,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type {
  CockpitAction,
  CurationRecord,
  InboxModel,
  InboxRow,
  SnoozeDuration,
  Urgency,
} from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "../components/context-menu.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../components/dialog.tsx";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from "../components/item.tsx";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "../components/popover.tsx";
import { ScrollArea } from "../components/scroll-area.tsx";
import { ToggleGroup, ToggleGroupItem } from "../components/toggle-group.tsx";
import {
  age,
  linkKinds,
  sessionKinds,
  urgencyText,
} from "./inbox/format.ts";

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
  onAction(action: CockpitAction): string | void;
  onSelectSession(sessionId: string): void;
  onSelectPlan?(planId: string): void;
  onOpenCurate?(row: InboxRow): void;
  pendingActionIds?: readonly string[];
  completedActionIds?: readonly string[];
}

function copy(text: string): void {
  void navigator.clipboard.writeText(text);
}

function openExternalUrl(url: string): void {
  window.open(url, "_blank", "noopener,noreferrer");
}

function selectRowSession(row: InboxRow, h: Handlers): void {
  if (row.planId !== null) h.onSelectPlan?.(row.planId);
  if (row.sessionId !== null) h.onSelectSession(row.sessionId);
}

/** Left click, per kind (TUI inbox.rs jump). */
function jump(row: InboxRow, h: Handlers): void {
  if (row.kind === "engine-down") {
    h.onAction({ kind: "engine.start" });
    return;
  }
  if (row.kind === "curate-retire") {
    selectRowSession(row, h);
    h.onOpenCurate?.(row);
    return;
  }
  if (row.kind === "review-requested") {
    if (row.url !== null) {
      copy(row.url);
      openExternalUrl(row.url);
    }
    return;
  }
  if (row.kind === "deploy") {
    if (row.url !== null) openExternalUrl(row.url);
    return;
  }
  if (row.sessionId !== null && sessionKinds.includes(row.kind)) {
    selectRowSession(row, h);
    // pro-d63: opening an unread row's chat reads it, so the item is dismissed (a synthetic row, id < 0, cannot be)
    if (row.kind === "unread" && row.id >= 0) h.onAction({ kind: "inbox.dismiss", itemId: row.id });
    return;
  }
  if (row.url !== null) {
    openExternalUrl(row.url);
  }
}

const PRIMARY_ACTION_KINDS: ReadonlySet<CockpitAction["kind"]> = new Set([
  "pr.merge",
  "pr.post",
  "pr.repost",
  "inbox.approve",
  "inbox.claim",
  "engine.start",
]);

function primaryActionOf(
  row: InboxRow,
): { readonly label: string; readonly action: CockpitAction } | null {
  const fromRow = row.actions.find((a) =>
    PRIMARY_ACTION_KINDS.has(a.action.kind),
  );
  if (fromRow !== undefined) return fromRow;
  if (row.kind === "engine-down") {
    return { label: "Start", action: { kind: "engine.start" } };
  }
  if (row.kind === "deploy") {
    return {
      label: "Approve",
      action: { kind: "inbox.approve", itemId: row.id },
    };
  }
  if (row.kind === "review-requested") {
    return { label: "Claim", action: { kind: "inbox.claim", itemId: row.id } };
  }
  if (row.kind === "pr-ready") {
    const first = row.label.split("\n")[0] ?? "";
    const space = first.indexOf(" ");
    const status = space === -1 ? "" : first.slice(space + 1);
    const pull =
      typeof row.subject === "string" && row.subject.startsWith("pr:")
        ? row.subject.slice(3)
        : undefined;
    const workflow = row.workflow ?? undefined;
    if (
      status === "ready to merge" &&
      (pull !== undefined || (row.planId !== null && workflow !== undefined))
    ) {
      return {
        label: "Merge",
        action: {
          kind: "pr.merge",
          ...(row.planId !== null ? { planId: row.planId } : {}),
          ...(workflow !== undefined ? { workflow } : {}),
          ...(pull !== undefined ? { pull } : {}),
        },
      };
    }
    if (
      status === "ready for user review" &&
      row.planId !== null &&
      (pull !== undefined || workflow !== undefined)
    ) {
      return {
        label: "Post",
        action: {
          kind: "pr.post",
          planId: row.planId,
          ...(workflow !== undefined ? { workflow } : {}),
          ...(pull !== undefined ? { pull } : {}),
        },
      };
    }
    if (
      status === "review overdue" &&
      row.planId !== null &&
      (pull !== undefined || workflow !== undefined)
    ) {
      return {
        label: "RePost",
        action: {
          kind: "pr.repost",
          planId: row.planId,
          ...(workflow !== undefined ? { workflow } : {}),
          ...(pull !== undefined ? { pull } : {}),
        },
      };
    }
  }
  return null;
}

function Menu(props: { row: InboxRow } & Handlers): ReactElement {
  const { row, onAction, onSelectSession } = props;
  const { id, sessionId, url } = row;
  const primary = primaryActionOf(row);
  const own: ReactElement[] = [];
  if (primary !== null && row.kind === "pr-ready") {
    own.push(
      <ContextMenuItem
        key={primary.label}
        onSelect={() => onAction(primary.action)}
      >
        {primary.label}
      </ContextMenuItem>,
    );
  }
  if (
    sessionId !== null &&
    (row.kind === "pr-ready" || row.kind === "pr-closed")
  ) {
    own.push(
      <ContextMenuItem key="lead" onSelect={() => selectRowSession(row, props)}>
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
  if (url !== null && row.kind === "dialog") {
    own.push(
      <ContextMenuItem key="open-link" onSelect={() => openExternalUrl(url)}>
        Open Link
      </ContextMenuItem>,
      <ContextMenuItem key="link" onSelect={() => copy(url)}>
        Copy Link
      </ContextMenuItem>,
    );
  }
  return (
    <ContextMenuContent>
      {own}
      {own.length > 0 ? <ContextMenuSeparator /> : null}
      {row.kind === "deploy" ? (
        <ContextMenuItem
          onSelect={() => onAction({ kind: "inbox.approve", itemId: id })}
        >
          Approve
        </ContextMenuItem>
      ) : null}
      {row.kind === "review-requested" ? (
        <ContextMenuItem
          onSelect={() => onAction({ kind: "inbox.claim", itemId: id })}
        >
          Claim
        </ContextMenuItem>
      ) : null}
      {SNOOZES.map(([label, duration]) => (
        <ContextMenuItem
          key={duration}
          onSelect={() =>
            onAction({ kind: "inbox.snooze", itemId: id, duration })
          }
        >
          {label}
        </ContextMenuItem>
      ))}
      <ContextMenuItem
        onSelect={() => onAction({ kind: "inbox.dismiss", itemId: id })}
      >
        Dismiss
      </ContextMenuItem>
    </ContextMenuContent>
  );
}

function RowIcon(props: { kind: string; urgency: Urgency }): ReactElement {
  const { kind, urgency } = props;
  const cls = cn("size-4 shrink-0", urgencyText[urgency]);
  if (kind === "deploy") return <RocketIcon className={cls} />;
  if (kind === "pr-ready" || kind === "pr-closed")
    return <GitPullRequestIcon className={cls} />;
  if (kind === "review-requested")
    return <GitPullRequestArrowIcon className={cls} />;
  if (kind === "curate-retire") return <SparklesIcon className={cls} />;
  if (urgency === "red") return <CircleAlertIcon className={cls} />;
  if (urgency === "yellow") return <CircleDotIcon className={cls} />;
  if (urgency === "magenta") return <CircleHelpIcon className={cls} />;
  if (urgency === "blue") return <EyeIcon className={cls} />;
  if (urgency === "cyan") return <BellIcon className={cls} />;
  return <MessageSquareIcon className={cls} />;
}

function InboxItemRow(props: { row: InboxRow } & Handlers): ReactElement {
  const {
    row,
    onAction,
    onOpenCurate,
    pendingActionIds = [],
    completedActionIds = [],
  } = props;
  const [activeActionId, setActiveActionId] = useState<string | null>(null);
  const [first = "", ...rest] = row.label.split("\n");
  const second = rest.join(" ").trim();
  const linked = row.url !== null && linkKinds.includes(row.kind);
  const primary = primaryActionOf(row);
  const startAsyncAction = (action: CockpitAction): void => {
    const id = onAction(action);
    if (typeof id === "string") setActiveActionId(id);
  };
  const buttonAction: {
    readonly label: string;
    readonly run: () => void;
  } | null =
    primary !== null
      ? { label: primary.label, run: () => startAsyncAction(primary.action) }
      : row.kind === "curate-retire" && row.contextId !== null
        ? { label: "Curate", run: () => onOpenCurate?.(row) }
        : row.id >= 0
          ? {
              label: "Dismiss",
              run: () =>
                startAsyncAction({ kind: "inbox.dismiss", itemId: row.id }),
            }
          : null;
  const isActionPending =
    activeActionId !== null && pendingActionIds.includes(activeActionId);
  const isActionCompleted =
    activeActionId !== null && completedActionIds.includes(activeActionId);
  const item = (
    <Item
      variant="outline"
      size="sm"
      role="button"
      tabIndex={0}
      data-kind={row.kind}
      data-urgency={row.urgency}
      data-id={row.id}
      onClick={() => jump(row, props)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          jump(row, props);
        }
      }}
      className="cursor-pointer hover:bg-accent/50"
    >
      <ItemMedia className="flex-col items-center justify-between gap-1 self-stretch group-has-[[data-slot=item-description]]/item:translate-y-0 group-has-[[data-slot=item-description]]/item:self-stretch">
        <div className="flex h-[19.25px] items-center justify-center">
          <RowIcon kind={row.kind} urgency={row.urgency} />
        </div>
        <span className="mt-auto block text-sm leading-normal">
          <span
            data-part="age"
            className="font-mono text-[10px] text-muted-foreground"
          >
            {age(row.ageSeconds)}
          </span>
        </span>
      </ItemMedia>
      <ItemContent className="justify-between self-stretch">
        <ItemTitle className={urgencyText[row.urgency]}>
          {linked && row.url !== null ? (
            <a
              href={row.url}
              target="_blank"
              rel="noopener noreferrer"
              onClick={(e) => e.stopPropagation()}
              className="hover:underline"
            >
              {first}
            </a>
          ) : (
            first
          )}
        </ItemTitle>
        {second === "" ? null : <ItemDescription>{second}</ItemDescription>}
      </ItemContent>
      <ItemActions className="flex-col items-end justify-between gap-1.5 self-stretch">
        {linked && row.url !== null ? (
          <a
            href={row.url}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Open external link"
            data-part="item-link"
            onClick={(e) => e.stopPropagation()}
            className="mt-[1.5px] rounded-xs text-muted-foreground transition-colors hover:text-fg-strong"
          >
            <ExternalLinkIcon className="size-4" />
          </a>
        ) : (
          <ChevronRightIcon className="mt-[1.5px] size-4 text-muted-foreground" />
        )}
        {buttonAction === null || isActionCompleted ? null : (
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-part="item-action"
            disabled={isActionPending}
            onClick={(e) => {
              e.stopPropagation();
              if (!isActionPending) buttonAction.run();
            }}
            className="h-6 gap-1 px-2 text-xs"
          >
            {isActionPending ? (
              <LoaderCircleIcon className="size-3 animate-spin" />
            ) : null}
            <span>{buttonAction.label}</span>
          </Button>
        )}
      </ItemActions>
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
  onAction(action: CockpitAction): string | void;
  onSelectSession(sessionId: string): void;
  onSelectPlan?(planId: string): void;
  pendingActionIds?: readonly string[];
  completedActionIds?: readonly string[];
}): ReactElement {
  const {
    inbox,
    onAction,
    onSelectSession,
    onSelectPlan,
    pendingActionIds,
    completedActionIds,
  } = props;
  const [selectedTiers, setSelectedTiers] = useState<
    "all" | ReadonlySet<Urgency>
  >("all");
  const [curateRow, setCurateRow] = useState<InboxRow | null>(null);
  const [checkedIds, setCheckedIds] = useState<ReadonlySet<string>>(new Set());
  const openCurate = (row: InboxRow): void => {
    const records: readonly CurationRecord[] = row.curation ?? [];
    setCheckedIds(new Set(records.map((r) => r.id)));
    setCurateRow(row);
  };
  const [collapsed, setCollapsed] = useState(false);
  const headerRef = useRef<HTMLDivElement | null>(null);
  const measureRef = useRef<HTMLDivElement | null>(null);
  const shell = "relative flex h-full min-h-0 w-full flex-col overflow-hidden";
  const heading =
    "text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";

  const rows = inbox?.rows ?? [];
  const counts = new Map<Urgency, number>();
  for (const t of TIERS) counts.set(t.urgency, 0);
  for (const r of rows) counts.set(r.urgency, (counts.get(r.urgency) ?? 0) + 1);

  const allSelected = selectedTiers === "all";
  const activeValues: string[] = allSelected
    ? ["all", ...TIERS.map((t) => t.urgency)]
    : Array.from(selectedTiers);

  const toggleTier = (u: Urgency): void => {
    if (selectedTiers === "all") {
      setSelectedTiers(new Set<Urgency>([u]));
      return;
    }
    const next = new Set(selectedTiers);
    if (next.has(u)) {
      next.delete(u);
    } else {
      next.add(u);
    }
    if (next.size === 0 || next.size === TIERS.length) {
      setSelectedTiers("all");
      return;
    }
    setSelectedTiers(next);
  };

  const filteredRows =
    selectedTiers === "all"
      ? rows
      : rows.filter((r) => selectedTiers.has(r.urgency));

  const countsKey = TIERS.map((t) => counts.get(t.urgency) ?? 0).join(",");

  useLayoutEffect(() => {
    const headerEl = headerRef.current;
    const measureEl = measureRef.current;
    if (!headerEl || !measureEl) return;
    const check = (): void => {
      const available = headerEl.clientWidth;
      const required = measureEl.scrollWidth;
      if (available > 0 && required > 0) {
        setCollapsed(available < required);
      }
    };
    check();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(check);
    ro.observe(headerEl);
    ro.observe(measureEl);
    return () => ro.disconnect();
  }, [countsKey, rows.length]);

  useEffect(() => {
    const headerEl = headerRef.current;
    const measureEl = measureRef.current;
    if (!headerEl || !measureEl) return;
    const available = headerEl.clientWidth;
    const required = measureEl.scrollWidth;
    if (available > 0 && required > 0) {
      setCollapsed(available < required);
    }
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
        <span className="font-mono text-[10px] opacity-80">{rows.length}</span>
      </ToggleGroupItem>
      {TIERS.map((t) => {
        const count = counts.get(t.urgency) ?? 0;
        return (
          <ToggleGroupItem
            key={t.urgency}
            value={t.urgency}
            data-tier-filter={t.label}
            data-urgency={t.urgency}
            onClick={() => toggleTier(t.urgency)}
            className={cn("h-6 gap-1.5 px-1.5 text-xs", t.textColor)}
          >
            <span>{t.label}</span>
            <span className="font-mono text-[10px] opacity-80">{count}</span>
          </ToggleGroupItem>
        );
      })}
    </>
  );

  return (
    <aside aria-label="Inbox" className={shell}>
      <div
        ref={measureRef}
        aria-hidden="true"
        className="pointer-events-none invisible absolute top-0 left-0 -z-10 flex h-8 w-max items-center gap-8 px-2.5"
      >
        <span className={cn(heading, "shrink-0")}>Inbox</span>
        <div className="flex shrink-0 items-center">
          <div className="inline-flex h-6 items-center px-1.5 text-xs">
            <span>all</span>
            <span className="ml-1.5 font-mono text-[10px]">{rows.length}</span>
          </div>
          {TIERS.map((t) => (
            <div
              key={t.urgency}
              className="inline-flex h-6 items-center px-1.5 text-xs"
            >
              <span>{t.label}</span>
              <span className="ml-1.5 font-mono text-[10px]">
                {counts.get(t.urgency) ?? 0}
              </span>
            </div>
          ))}
        </div>
      </div>
      <div
        ref={headerRef}
        data-part="inbox-header"
        className="flex h-8 shrink-0 items-center justify-between gap-8 px-2.5"
      >
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
                <span>
                  {allSelected
                    ? "all"
                    : `${selectedTiers.size} tier${selectedTiers.size === 1 ? "" : "s"}`}
                </span>
                <span className="font-mono text-[10px] opacity-80">
                  {filteredRows.length}
                </span>
              </Button>
            </PopoverTrigger>
            <PopoverContent
              align="end"
              className="w-auto min-w-[8.5rem] p-1.5"
            >
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
      ) : filteredRows.length === 0 ? (
        <p data-empty className="px-[14px] py-3 text-muted-foreground">
          up to date, drink some water
        </p>
      ) : (
        <ScrollArea data-part="inbox-scroll" className="min-h-0 flex-1 [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:w-full [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:!min-w-0">
        <ItemGroup className="gap-2 px-2.5 py-1.5">
          {filteredRows.map((row) => (
            <InboxItemRow
              key={row.id}
              row={row}
              onAction={onAction}
              onSelectSession={onSelectSession}
              {...(onSelectPlan !== undefined ? { onSelectPlan } : {})}
              {...(pendingActionIds !== undefined ? { pendingActionIds } : {})}
              {...(completedActionIds !== undefined
                ? { completedActionIds }
                : {})}
              onOpenCurate={openCurate}
            />
          ))}
        </ItemGroup>
        </ScrollArea>
      )}
      <Dialog
        open={curateRow !== null}
        onOpenChange={(open) => {
          if (!open) setCurateRow(null);
        }}
      >
        <DialogContent className="overflow-hidden">
          <DialogHeader>
            <DialogTitle>
              {curateRow?.contextId
                ? `Retire records of ${curateRow.contextId}`
                : "Retire records"}
            </DialogTitle>
            <DialogDescription>
              Checked records will be retired; unchecked records will be kept.
            </DialogDescription>
          </DialogHeader>
          <ScrollArea className="[&>[data-slot=scroll-area-viewport]]:max-h-64">
          <div className="flex flex-col gap-2 py-1">
            {(curateRow?.curation ?? []).length === 0 ? (
              <p className="text-xs text-muted-foreground">
                No pending records to retire.
              </p>
            ) : (
              (curateRow?.curation ?? []).map((rec) => {
                const checked = checkedIds.has(rec.id);
                return (
                  <label
                    key={rec.id}
                    className="flex cursor-pointer items-start gap-2.5 rounded-md border border-border p-2 text-xs hover:bg-accent/40"
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => {
                        const next = new Set(checkedIds);
                        if (next.has(rec.id)) next.delete(rec.id);
                        else next.add(rec.id);
                        setCheckedIds(next);
                      }}
                      className="mt-0.5"
                    />
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <div className="font-mono font-medium text-fg-strong">
                        {rec.id}{" "}
                        <span className="font-normal text-muted-foreground">
                          — {rec.detail}
                        </span>
                      </div>
                      <div className="text-muted-foreground">{rec.text}</div>
                    </div>
                  </label>
                );
              })
            )}
          </div>
          </ScrollArea>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setCurateRow(null)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={() => {
                if (curateRow?.contextId) {
                  const all = curateRow.curation ?? [];
                  const approve = all
                    .filter((r) => checkedIds.has(r.id))
                    .map((r) => r.id)
                    .join(",");
                  const reject = all
                    .filter((r) => !checkedIds.has(r.id))
                    .map((r) => r.id)
                    .join(",");
                  if (approve !== "" || reject !== "") {
                    onAction({
                      kind: "context.retire",
                      contextId: curateRow.contextId,
                      approve,
                      reject,
                    });
                  } else if (curateRow.id >= 0) {
                    onAction({ kind: "inbox.dismiss", itemId: curateRow.id });
                  }
                }
                setCurateRow(null);
              }}
            >
              Confirm
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </aside>
  );
}
