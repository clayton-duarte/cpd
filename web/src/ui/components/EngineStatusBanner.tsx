import { CircleAlertIcon } from "lucide-react";
import * as React from "react";
import { Alert, AlertDescription, AlertTitle } from "./alert.tsx";

export interface EngineStatusBannerProps {
  readonly engineRunning: boolean;
  readonly engineDownSince: string | null; // RFC 3339
  readonly engineRestartReason: string | null; // "install" while `cpd install` restarts the engine
  readonly now: Date; // injected clock
}

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

function since(from: string, now: Date): string | null {
  const ms = now.getTime() - Date.parse(from);
  if (!Number.isFinite(ms)) return null;
  const mins = Math.max(0, Math.floor(ms / 60_000));
  if (mins < 1) return "less than a minute";
  if (mins < 60) return plural(mins, "minute");
  if (mins < 1440) return plural(Math.floor(mins / 60), "hour");
  return plural(Math.floor(mins / 1440), "day");
}

/** Banner shown while the engine is down or restarting; nothing while it runs. */
export function EngineStatusBanner(props: EngineStatusBannerProps): React.ReactElement | null {
  if (props.engineRunning) return null;
  const restarting = props.engineRestartReason === "install";
  const age = props.engineDownSince === null ? null : since(props.engineDownSince, props.now);
  return (
    <Alert data-part="engine-status-banner">
      <CircleAlertIcon className="size-4" />
      <AlertTitle>{restarting ? "Engine restarting after install" : "Engine is down"}</AlertTitle>
      <AlertDescription>{age === null ? "Down since an unknown time." : `Down for ${age}.`}</AlertDescription>
    </Alert>
  );
}
