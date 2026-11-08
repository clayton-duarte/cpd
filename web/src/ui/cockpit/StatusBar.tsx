import { BookOpenIcon, DollarSignIcon, LayoutDashboardIcon, ListTreeIcon, ScrollTextIcon, SettingsIcon } from "lucide-react";
import type { ReactElement } from "react";
import type { StatusModel as WireStatus } from "../../protocol/cockpit.ts";
import type { Runner, SessionState, Stat } from "../../protocol/domain.ts";
import { cn } from "../../lib/utils.ts";
import { Badge } from "../components/badge.tsx";
import { Button } from "../components/button.tsx";
import { dotDisplay, glyphClass, sessionDisplay } from "../dango-status.ts";

/** The machine status the bar draws (pro-d40): the server's StatusModel with the incident as elapsed text. */
export interface StatusModel {
  readonly runner: "pi" | "durable" | null;
  readonly preview: string | null;
  /** The open incident: its elapsed text (`5m`, `2h 5m`, `3d`) and its Lead's Session id; null when none is on. */
  readonly incident: { readonly elapsed: string; readonly lead: string | null } | null;
  readonly workers: { readonly queued: number; readonly ready: number; readonly tenants: number; readonly concurrency: number } | null;
  readonly version: string | null;
  readonly hostAlert: boolean;
  readonly mode: "production" | "preview";
  readonly drift: { readonly count: number; readonly oldestAt: string | null; readonly stale: boolean } | null;
}

/** `5m`, `2h 5m`, `3d`: the elapsed time since `startedAt`. */
export function elapsed(startedAt: string, nowMs: number): string {
  const minutes = Math.max(0, Math.floor((nowMs - Date.parse(startedAt)) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 === 0 ? `${hours}h` : `${hours}h ${minutes % 60}m`;
  return `${Math.floor(hours / 24)}d`;
}

/** The server's status as the bar draws it. */
export function barStatus(s: WireStatus, nowMs: number): StatusModel {
  return {
    runner: s.runner === "pi" || s.runner === "durable" ? s.runner : null,
    preview: s.preview,
    incident: s.incident === null ? null : { elapsed: elapsed(s.incident.startedAt, nowMs), lead: s.incident.lead },
    workers: s.workers === null ? null : { queued: s.workers.queued, ready: s.workers.waiting, tenants: s.workers.tenants, concurrency: s.workers.limit },
    version: s.version,
    hostAlert: s.hostAlert,
    mode: s.mode,
    drift: s.drift,
  };
}

export interface StatusBarProps {
  /** The machine status; until it arrives the bar shows the selected session instead (`runner`, `state`, `version`). */
  readonly status?: StatusModel;
  readonly costsOn?: boolean;
  readonly onReset?: () => void;
  readonly onLegend?: () => void;
  readonly onDocs?: () => void;
  readonly onLogs?: () => void;
  readonly onCosts?: () => void;
  readonly onSettings?: () => void;
  readonly onSelectSession?: (sessionId: string) => void;
  readonly runner?: Runner | null;
  readonly state?: SessionState | null;
  readonly version?: Stat<string> | null;
}

const PILL = "font-mono text-meta";

function Pills({ status, onSettings, onSelectSession }: { status: StatusModel; onSettings?: () => void; onSelectSession?: (sessionId: string) => void }): ReactElement {
  const w = status.workers;
  const incident = status.incident;
  return (
    <>
      {status.runner === null ? null : <span data-part="runner" className={cn(PILL, "text-fg-strong")}>{status.runner}</span>}
      {status.preview === null ? null : <span data-part="preview" className={cn(PILL, "text-status-landing")}>{`preview: ${status.preview}`}</span>}
      <span className="flex-1" />
      {incident === null ? null : (
        <span
          data-part="incident"
          role="button"
          className={cn(PILL, "italic text-status-blocked-bright", incident.lead === null ? undefined : "cursor-pointer")}
          {...(incident.lead === null ? {} : { onClick: () => onSelectSession?.(incident.lead as string) })}
        >{`Incident Mode: ${incident.elapsed}`}</span>
      )}
      {w === null ? null : (
        <span
          data-part="workers"
          role="button"
          className={cn(PILL, "text-fg-dim")}
          onClick={onSettings}
        >{`workers ${w.queued}\u{f0798}${w.ready}\u{f0798}${w.tenants}/${w.concurrency}`}</span>
      )}
      {status.drift === null || status.drift.count === 0 ? null : (
        <Badge
          data-part="drift"
          variant="outline"
          {...(status.drift.stale ? { "data-stale": "true" } : {})}
          className={cn(PILL, status.drift.stale ? "text-mitarashi" : "text-fg-dim")}
        >{`drift ${status.drift.count}`}</Badge>
      )}
      {status.version === null ? null : (
        <Badge
          data-part="version"
          variant="ghost"
          {...(status.hostAlert ? { "data-alert": "true" } : {})}
          className={cn(PILL, status.hostAlert ? "text-status-blocked" : "text-fg-dim")}
        >{status.version}</Badge>
      )}
      {status.mode === "preview" ? <Badge data-part="mode" variant="outline" className={cn(PILL, "border-sakura text-sakura")}>preview</Badge> : null}
    </>
  );
}

function SessionPills(props: Pick<StatusBarProps, "runner" | "state" | "version">): ReactElement {
  const { runner = null, state = null, version = null } = props;
  const d = state === null ? null : sessionDisplay[dotDisplay({ state, attention: null })];
  return (
    <>
      <span className="flex-1" />
      <span data-part="status-right" className="flex items-center gap-2 font-mono text-meta text-muted-foreground">
        <span data-part="runner" className={runner === null ? "text-fg-dim" : undefined}>Runner: {runner ?? "none"}</span>
        <span aria-hidden="true" className="text-line">│</span>
        <span
          data-part="state"
          {...(d === null ? {} : { "data-tone": d.tone, "data-glyph": d.glyph })}
          className={`flex items-center gap-1.5 ${state === null ? "text-fg-dim" : ""}`}
        >
          {d === null ? null : <span aria-hidden="true" className={glyphClass[d.glyph]} />}
          State: {state ?? "none"}
        </span>
        <span aria-hidden="true" className="text-line">│</span>
        <span data-part="version">cpd: {version !== null && version.state === "known" ? version.value : "unknown"}</span>
      </span>
    </>
  );
}

export function StatusBar(props: StatusBarProps): ReactElement {
  const { status, costsOn = false, onReset, onLegend, onDocs, onLogs, onCosts, onSettings } = props;
  const buttons = [
    { label: "Reset layout", icon: <LayoutDashboardIcon />, click: onReset, part: "reset-layout" },
    { label: "Legend", icon: <ListTreeIcon />, click: onLegend, part: "legend" },
    { label: "Documentation", icon: <BookOpenIcon />, click: onDocs, part: "docs" },
    { label: "Logs", icon: <ScrollTextIcon />, click: onLogs, part: "logs" },
    { label: "Costs", icon: <DollarSignIcon />, click: onCosts, part: "costs" },
    { label: "Settings", icon: <SettingsIcon />, click: onSettings, part: "settings" },
  ].filter((b) => b.click !== undefined);
  return (
    <div data-part="status-bar" className="flex h-full w-full items-center gap-2 overflow-hidden px-2 text-[11.5px]">
      <span className="flex items-center gap-0.5">
        {buttons.map((b) => (
          <Button
            key={b.part}
            type="button"
            variant="ghost"
            size="icon-xs"
            data-part={b.part}
            aria-label={b.label}
            {...(b.part === "costs" ? { "aria-pressed": costsOn } : {})}
            onClick={b.click}
            className="text-fg-strong"
          >
            {b.icon}
          </Button>
        ))}
      </span>
      {status === undefined ? <SessionPills runner={props.runner} state={props.state} version={props.version} /> : <Pills status={status} onSettings={onSettings} onSelectSession={props.onSelectSession} />}
    </div>
  );
}
