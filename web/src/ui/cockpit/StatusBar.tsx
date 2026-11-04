import type { ReactElement } from "react";
import type { Runner, SessionState, Stat } from "../../protocol/domain.ts";

export interface StatusBarProps {
  readonly runner: Runner | null;
  readonly state: SessionState | null;
  readonly version: Stat<string> | null;
  readonly showCost?: boolean;
  readonly onToggleCost?: () => void;
  readonly onResetLayout?: () => void;
}

// Stub: showCost, onToggleCost and onResetLayout are not rendered yet.
export function StatusBar(props: StatusBarProps): ReactElement {
  const { runner, state, version } = props;
  return (
    <>
      <span data-part="runner">Runner: {runner ?? "none"}</span>{" "}
      <span data-part="state">State: {state ?? "none"}</span>{" "}
      <span data-part="version">cpd: {version !== null && version.state === "known" ? version.value : "unknown"}</span>
    </>
  );
}
