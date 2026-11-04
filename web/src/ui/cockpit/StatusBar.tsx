import type { ReactElement } from "react";
import type { Runner, Stat } from "../../protocol/domain.ts";

export function StatusBar(props: { runner: Runner | null; state: string | null; version: Stat<string> | null }): ReactElement {
  const { runner, state, version } = props;
  return (
    <>
      <span data-part="runner">Runner: {runner ?? "none"}</span>{" "}
      <span data-part="state">State: {state ?? "none"}</span>{" "}
      <span data-part="version">cpd: {version !== null && version.state === "known" ? version.value : "unknown"}</span>
    </>
  );
}
