import type { ReactElement } from "react";
import type { CockpitAction, InboxModel, InboxRow, SnoozeDuration } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "../components/context-menu.tsx";
import { LINK_CUE, age, glyphOf, linkKinds, sessionKinds, urgencyText } from "./inbox/format.ts";

const SNOOZES: readonly [string, SnoozeDuration][] = [
  ["Snooze 15 min", "15m"],
  ["Snooze 1 h", "1h"],
  ["Snooze until tomorrow 09:00", "tomorrow"],
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
  const { id, planId, workflow, sessionId, url } = row;
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
  const shell = "flex h-full min-h-0 w-full flex-col overflow-hidden";
  const heading = "text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";
  return (
    <aside aria-label="Inbox" className={shell}>
      <div data-part="inbox-header" className="flex h-9 shrink-0 items-center gap-2 px-[14px]">
        <h2 className={heading}>Inbox</h2>
        {inbox === null ? null : <span className="font-mono text-meta text-fg-strong">{inbox.rows.length}</span>}
      </div>
      {inbox === null ? (
        <p className="px-[14px] text-fg-dim">Loading inbox…</p>
      ) : inbox.rows.length === 0 ? (
        <p data-empty className="px-[14px] py-3 text-muted-foreground">
          up to date, drink some water
        </p>
      ) : (
        <ul className="m-0 flex min-h-0 flex-1 list-none flex-col gap-0.5 overflow-y-auto p-0 px-2">
          {inbox.rows.map((row) => (
            <Item key={row.id} row={row} onAction={onAction} onSelectSession={onSelectSession} />
          ))}
        </ul>
      )}
    </aside>
  );
}
