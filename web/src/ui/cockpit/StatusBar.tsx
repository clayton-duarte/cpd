import type { ReactElement } from "react";
import type { Runner, SessionState, Stat } from "../../protocol/domain.ts";
import { dotDisplay, glyphClass, sessionDisplay } from "../dango-status.ts";

/** The machine status the server sends (pro-d40). Wire shape fixed by bar-t-ui's tests. */
export interface StatusModel {
  readonly runner: "pi" | "durable" | null;
  readonly preview: string | null;
  /** Elapsed text of an open incident (`5m`, `2h 5m`, `3d`); null when no incident is on. */
  readonly incident: string | null;
  readonly workers: { readonly queued: number; readonly ready: number; readonly tenants: number; readonly concurrency: number } | null;
  readonly version: string | null;
  readonly hostAlert: boolean;
}

export interface StatusBarProps {
  // Old props stay optional until the implementation Job moves App to `status`.
  readonly runner?: Runner | null;
  readonly state?: SessionState | null;
  readonly version?: Stat<string> | null;
  readonly status?: StatusModel;
  readonly costsOn?: boolean;
  readonly onReset?: () => void;
  readonly onLegend?: () => void;
  readonly onLogs?: () => void;
  readonly onCosts?: () => void;
  readonly onSettings?: () => void;
  readonly showCost?: boolean;
  readonly onToggleCost?: () => void;
  readonly onResetLayout?: () => void;
}

const BTN = "h-[22px] rounded-[4px] px-2 text-fg-strong hover:bg-row-hover";

export function StatusBar(props: StatusBarProps): ReactElement {
  const { runner = null, state = null, version = null, showCost = false, onToggleCost, onResetLayout } = props;
  const d = state === null ? null : sessionDisplay[dotDisplay({ state, attention: null })];
  return (
    <div data-part="status-bar" className="flex h-full w-full items-center gap-1 overflow-hidden px-2 text-[11.5px]">
      {onResetLayout === undefined ? null : (
        <button type="button" data-part="reset-layout" onClick={onResetLayout} className={BTN}>Reset layout</button>
      )}
      {onToggleCost === undefined ? null : (
        <button
          type="button"
          data-part="cost-toggle"
          aria-pressed={showCost}
          onClick={onToggleCost}
          className={`${BTN} font-mono ${showCost ? "bg-surface-pressed" : ""}`}
        >
          $
        </button>
      )}
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
    </div>
  );
}
