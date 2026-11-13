import * as React from "react";

export interface EngineStatusBannerProps {
  readonly engineRunning: boolean;
  readonly engineDownSince: string | null; // RFC 3339
  readonly engineRestartReason: string | null; // "install" while `cpd install` restarts the engine
  readonly now: Date; // injected clock
}

/** Banner shown while the engine is down or restarting. Stub: always renders an empty alert (wrong on purpose). */
export function EngineStatusBanner(_props: EngineStatusBannerProps): React.ReactElement | null {
  return <div role="alert" data-slot="engine-status-banner" />;
}
