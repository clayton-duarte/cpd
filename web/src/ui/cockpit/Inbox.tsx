import { useState, type ReactElement } from "react";
import type { CockpitAction, InboxModel, InboxRow, SnoozeDuration, Urgency } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { ButtonGroup } from "../components/button-group.tsx";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "../components/context-menu.tsx";
import { LINK_CUE, age, glyphOf, linkKinds, sessionKinds, urgencyText } from "./inbox/format.ts";

const SNOOZES: readonly [string, SnoozeDuration][] = [
  ["Snooze 15 min", "15m"],
  ["Snooze 1 h", "1h"],
  ["Snooze until tomorrow 09:00", "tomorrow"],
];

interface TierSpec {
  readonly urgency: Urgency;
  readonly label: string;
  readonly filled: string;
  readonly outlined: string;
}

const TIERS: readonly TierSpec[] = [
  {
    urgency: "red",
    label: "blocker",
    filled:
      "border-status-blocked bg-status-blocked text-background hover:bg-status-blocked-bright hover:text-background",
    outlined:
      "border-status-blocked/60 bg-transparent text-status-blocked-bright hover:bg-status-blocked/15 hover:text-status-blocked-bright",
  },
  {
    urgency: "yellow",
    label: "delivery",
    filled:
      "border-status-landing bg-status-landing text-background hover:bg-status-landing-bright hover:text-background",
    outlined:
      "border-status-landing/60 bg-transparent text-status-landing-bright hover:bg-status-landing/15 hover:text-status-landing-bright",
  },
  {
    urgency: "magenta",
    label: "decision",
    filled:
      "border-status-decision bg-status-decision text-background hover:bg-status-decision-bright hover:text-background",
    outlined:
      "border-status-decision/60 bg-transparent text-status-decision-bright hover:bg-status-decision/15 hover:text-status-decision-bright",
  },
  {
    urgency: "blue",
    label: "support",
    filled:
      "border-status-focus bg-status-focus text-background hover:bg-status-focus-bright hover:text-background",
    outlined:
      "border-status-focus/60 bg-transparent text-status-focus-bright hover:bg-status-focus/15 hover:text-status-focus-bright",
  },
  {
    urgency: "cyan",
    label: "notice",
    filled:
      "border-status-active bg-status-active text-background hover:bg-status-active-bright hover:text-background",
    outlined:
      "border-status-active/60 bg-transparent text-status-active-bright hover:bg-status-active/15 hover:text-status-active-bright",
  },
  {
    urgency: "white",
    label: "unread",
    filled:
      "border-fg-strong bg-fg-strong text-background hover:bg-fg-strong/90 hover:text-background",
    outlined:
      "border-fg-dim/60 bg-transparent text-fg-strong hover:bg-fg-strong/15 hover:text-fg-strong",
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

function Item(props: { row: InboxRow } & Handlers): ReactElement {
  const { row } = props;
  const [first = "", ...rest] = row.label.split("\n");
  const second = rest.join(" ").trim();
  const linked = row.url !== null && linkKinds.includes(row.kind);
  const li = (
    <li
      data-kind={row.kind}
      data-urgency={row.urgency}
      data-id={row.id}
      onClick={() => jump(row, props)}
      className="row-item flex cursor-default flex-col rounded-[7px] px-2 py-1 hover:bg-row-hover"
    >
      <div data-part="line1" className="flex w-full items-baseline gap-2 font-mono text-key">
        <span aria-hidden="true" data-part="glyph" className={cn("shrink-0", urgencyText[row.urgency])}>
          {glyphOf(row.kind, row.urgency)}
        </span>
        {linked ? (
          <span className="min-w-0 flex-1 truncate text-fg-strong">
            <a href={row.url ?? undefined} target="_blank" rel="noreferrer" onClick={(e) => e.stopPropagation()} className="underline">
              {first}
            </a>
            {LINK_CUE}
          </span>
        ) : (
          <span className="min-w-0 flex-1 truncate text-fg-strong">{first}</span>
        )}
        <span data-part="age" className="shrink-0 text-meta text-muted-foreground">
          {age(row.ageSeconds)}
        </span>
      </div>
      {second === "" ? null : (
        <div data-part="line2" className="truncate pl-5 text-[12px] text-muted-foreground">
          <span>{second}</span>
        </div>
      )}
    </li>
  );
  if (row.kind === "engine-down") return li;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{li}</ContextMenuTrigger>
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
  const [selected, setSelected] = useState<"all" | ReadonlySet<Urgency>>("all");
  const shell = "flex h-full min-h-0 w-full flex-col overflow-hidden";
  const heading = "text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";
  const rows = inbox?.rows ?? [];
  const count = (u: Urgency): number => rows.filter((r) => r.urgency === u).length;
  const isAll = selected === "all";
  const toggle = (u: Urgency): void => {
    const next = new Set(selected === "all" ? [] : selected);
    if (selected === "all" || !next.has(u)) next.add(u);
    else next.delete(u);
    setSelected(next.size === 0 || next.size === TIERS.length ? "all" : next);
  };
  const shown = selected === "all" ? rows : rows.filter((r) => selected.has(r.urgency));
  const chip = "h-6 px-1.5 text-[11px] font-medium shadow-none";
  const Count = (p: { n: number }): ReactElement => (
    <span data-part="filter-count" className="font-mono">
      {p.n}
    </span>
  );
  return (
    <aside aria-label="Inbox" className={shell}>
      <div data-part="inbox-header" className="flex flex-wrap items-center gap-x-2 gap-y-1 px-2.5 py-1.5 shrink-0">
        <h2 className={heading}>Inbox</h2>
        {inbox === null ? null : <span className="font-mono text-meta text-fg-strong">{shown.length}</span>}
        <ButtonGroup aria-label="Filter inbox by tier" data-part="inbox-filters" className="ml-auto w-fit">
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-tier-filter="all"
            aria-pressed={isAll}
            onClick={() => setSelected("all")}
            className={cn(
              chip,
              isAll
                ? "border-fg-strong bg-fg-strong text-background hover:bg-fg-strong/90 hover:text-background"
                : "border-fg-dim/60 bg-transparent text-fg-strong hover:bg-fg-strong/15 hover:text-fg-strong",
            )}
          >
            <span>all</span> <Count n={rows.length} />
          </Button>
          {TIERS.map((t) => {
            const active = isAll || selected.has(t.urgency);
            return (
              <Button
                key={t.urgency}
                type="button"
                variant="outline"
                size="sm"
                data-tier-filter={t.label}
                data-urgency={t.urgency}
                aria-pressed={active}
                onClick={() => toggle(t.urgency)}
                className={cn(chip, active ? t.filled : t.outlined)}
              >
                <span>{t.label}</span> <Count n={count(t.urgency)} />
              </Button>
            );
          })}
        </ButtonGroup>
      </div>
      {inbox === null ? (
        <p className="px-[14px] text-fg-dim">Loading inbox…</p>
      ) : shown.length === 0 ? (
        <p data-empty className="px-[14px] py-3 text-muted-foreground">
          {rows.length === 0 ? "up to date, drink some water" : "no items in this filter"}
        </p>
      ) : (
        <ul className="m-0 flex min-h-0 flex-1 list-none flex-col gap-0.5 overflow-y-auto p-0 px-2">
          {shown.map((row) => (
            <Item key={row.id} row={row} onAction={onAction} onSelectSession={onSelectSession} />
          ))}
        </ul>
      )}
    </aside>
  );
}
