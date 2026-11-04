import { type KeyboardEvent, type ReactElement, useState } from "react";
import type { CockpitAction, InboxModel, InboxRow } from "../../protocol/cockpit.ts";
import { cn } from "../../lib/utils.ts";
import { Button } from "../components/button.tsx";
import { type InboxTier, glyphClass, inboxDisplay } from "../dango-status.ts";
import { age, displayOrder, tierText, urgencies } from "./inbox/format.ts";

const actionTone: Readonly<Record<string, string>> = {
  "inbox.approve": "border-status-landing/60 text-status-landing-bright",
  "inbox.claim": "border-status-focus/60 text-status-focus-bright",
};
const barButton = "h-6 rounded-[5px] px-2 disabled:opacity-50";

function Bar(props: { row: InboxRow; onSelectSession(id: string): void }): ReactElement {
  const { row, onSelectSession } = props;
  const sessionId = row.sessionId;
  return (
    <div data-part="bar" role="group" aria-label="Row actions" className="mt-0.5 flex gap-1 text-[11.5px]">
      <button
        type="button"
        data-part="bar-open"
        disabled={sessionId === null}
        onClick={() => (sessionId !== null ? onSelectSession(sessionId) : undefined)}
        className={cn(barButton, "bg-primary font-semibold text-primary-foreground")}
      >
        Open <kbd className="font-mono">⏎</kbd>
      </button>
      <button type="button" data-part="bar-snooze" disabled className={cn(barButton, "border border-edge text-fg-strong")}>
        Snooze <kbd className="font-mono text-muted-foreground">S</kbd>
      </button>
      <button type="button" data-part="bar-dismiss" disabled className={cn(barButton, "border border-edge text-fg-strong")}>
        Dismiss <kbd className="font-mono text-muted-foreground">E</kbd>
      </button>
    </div>
  );
}

function Row(props: {
  row: InboxRow;
  focused: boolean;
  onFocus(): void;
  onAction(action: CockpitAction): void;
  onSelectSession(id: string): void;
}): ReactElement {
  const { row, focused, onFocus, onAction, onSelectSession } = props;
  const d = inboxDisplay(row.urgency);
  const [first = "", ...rest] = row.label.split("\n");
  const headClass = "flex min-w-0 flex-col gap-px text-left";
  const headBody = (
    <>
      <span data-part="line1" className="flex w-full items-baseline gap-2">
        <strong data-part="subject" className="min-w-0 flex-1 truncate font-mono text-key font-normal text-fg-strong">
          {first}
        </strong>
        <span data-part="age" className="shrink-0 font-mono text-meta text-muted-foreground">
          {age(row.ageSeconds)}
        </span>
      </span>
      <span data-part="line2" className="flex w-full items-baseline gap-1.5 truncate text-[12px] text-muted-foreground">
        <span data-part="kind" className="font-mono text-[10.5px] text-fg-dim">
          {row.kind}
        </span>
        {rest.map((line, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: static label lines
          <span key={i}>{` ${line}`}</span>
        ))}
      </span>
    </>
  );
  return (
    <li
      data-kind={row.kind}
      data-urgency={row.urgency}
      data-id={row.id}
      data-tier={d.tier}
      data-focused={focused ? "true" : "false"}
      tabIndex={focused ? 0 : -1}
      onFocus={onFocus}
      onClick={onFocus}
      className={cn(
        "row-item flex gap-2.5 rounded-[7px] p-2 outline-none focus-visible:ring-1 focus-visible:ring-ring",
        focused ? "bg-status-focus/[.13]" : "hover:bg-row-hover",
      )}
    >
      <span aria-hidden="true" data-part="glyph" className={cn(glyphClass[d.glyph], "mt-1.5")} />
      <div data-part="body" className="flex min-w-0 flex-1 flex-col gap-[3px]">
        {row.sessionId !== null ? (
          <button
            type="button"
            data-action="open-session"
            className={headClass}
            onClick={() => onSelectSession(row.sessionId as string)}
          >
            {headBody}
          </button>
        ) : (
          <div data-part="head" className={headClass}>
            {headBody}
          </div>
        )}
        {row.url !== null ? (
          <a href={row.url} className="self-start font-mono text-[10.5px] text-status-focus-bright hover:underline">
            Link
          </a>
        ) : null}
        {row.actions.length > 0 ? (
          <div data-part="actions" className="flex flex-wrap gap-1.5">
            {row.actions.map((a) => (
              <Button
                key={a.label}
                type="button"
                variant="outline"
                size="sm"
                className={cn(
                  "h-6 rounded-[5px] px-2.5 text-[12px] font-semibold",
                  actionTone[a.action.kind] ?? "border-edge text-fg-strong",
                )}
                onClick={() => onAction(a.action)}
              >
                {a.label}
              </Button>
            ))}
          </div>
        ) : null}
        {focused ? <Bar row={row} onSelectSession={onSelectSession} /> : null}
      </div>
    </li>
  );
}

export function Inbox(props: {
  inbox: InboxModel | null;
  onAction(action: CockpitAction): void;
  onSelectSession(sessionId: string): void;
}): ReactElement {
  const { inbox, onAction, onSelectSession } = props;
  const [focusedId, setFocusedId] = useState<number | null>(null);
  const shell = "flex h-full min-h-0 w-full flex-col overflow-hidden";
  const heading = "text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground";
  if (inbox === null) {
    return (
      <aside aria-label="Inbox" className={shell}>
        <div data-part="inbox-header" className="flex h-9 shrink-0 items-center gap-2 px-[14px]">
          <h2 className={heading}>Inbox</h2>
        </div>
        <p className="px-[14px] text-fg-dim">Loading inbox…</p>
      </aside>
    );
  }
  const ordered = displayOrder(inbox.rows);
  const effective = ordered.find((r) => r.id === focusedId) ?? ordered[0];
  const tierCount = (t: InboxTier) => inbox.rows.filter((r) => inboxDisplay(r.urgency).tier === t).length;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>): void => {
    const target = e.target as HTMLElement;
    if (!(target instanceof HTMLLIElement) || !target.hasAttribute("data-kind")) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key.toLowerCase();
    const idx = effective === undefined ? -1 : ordered.indexOf(effective);
    if (k === "j" || k === "k") {
      const next = ordered[Math.max(0, Math.min(ordered.length - 1, idx + (k === "j" ? 1 : -1)))];
      if (next === undefined) return;
      e.preventDefault();
      setFocusedId(next.id);
      e.currentTarget.querySelector<HTMLElement>(`li[data-id="${next.id}"]`)?.focus();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (effective?.sessionId != null) onSelectSession(effective.sessionId);
    }
  };

  return (
    <aside aria-label="Inbox" className={shell}>
      <div data-part="inbox-header" className="flex h-9 shrink-0 items-center gap-2 px-[14px]">
        <h2 className={heading}>
          Inbox<span className="sr-only">{` (${inbox.openCount})`}</span>
        </h2>
        <span data-part="total" aria-hidden="true" className="font-mono text-meta text-fg-strong">
          {inbox.rows.length}
        </span>
        <span className="flex-1" />
        <div data-part="counts" aria-hidden="true" className="flex gap-2 font-mono text-meta">
          {urgencies.map((u) => {
            const d = inboxDisplay(u);
            const n = tierCount(d.tier);
            return (
              <span
                key={u}
                data-part="count"
                data-tier={d.tier}
                data-urgency={u}
                title={`${n} ${d.tierLabel}`}
                className={n === 0 ? "text-fg-dim" : tierText[d.tier]}
              >
                {n}
              </span>
            );
          })}
        </div>
      </div>
      {inbox.engineRunning ? null : (
        <p role="status" className="px-[14px] pb-1.5 text-meta text-status-blocked">
          Engine is not running
        </p>
      )}
      {inbox.rows.length === 0 ? (
        <p data-empty className="px-[14px] py-3 text-muted-foreground">
          Nothing needs you
        </p>
      ) : null}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: roving-focus list keys */}
      <div data-part="groups" onKeyDown={onKeyDown} className="flex min-h-0 flex-1 flex-col overflow-y-auto px-2">
        {urgencies.map((u) => {
          const d = inboxDisplay(u);
          const rows = ordered.filter((r) => r.urgency === u);
          if (rows.length === 0) return null;
          return (
            <section
              key={u}
              data-part="group"
              data-tier={d.tier}
              data-urgency={u}
              aria-labelledby={`inbox-group-${d.tier}`}
              className="mb-1.5 flex flex-col gap-0.5"
            >
              <h3
                data-part="group-label"
                id={`inbox-group-${d.tier}`}
                className={cn("px-2 pb-0.5 pt-1.5 font-mono text-meta", tierText[d.tier])}
              >
                {d.tierLabel}
              </h3>
              <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
                {rows.map((row) => (
                  <Row
                    key={row.id}
                    row={row}
                    focused={effective?.id === row.id}
                    onFocus={() => setFocusedId(row.id)}
                    onAction={onAction}
                    onSelectSession={onSelectSession}
                  />
                ))}
              </ul>
            </section>
          );
        })}
      </div>
      <div data-part="footer" className="shrink-0 border-t border-border px-[14px] py-2 font-mono text-meta text-fg-dim">
        J/K move · ⏎ open · S snooze · E dismiss
      </div>
    </aside>
  );
}
