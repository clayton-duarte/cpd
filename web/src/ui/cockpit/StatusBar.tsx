import type { ReactElement } from "react";
import type { Runner, Stat } from "../../protocol/domain.ts";

export function StatusBar(_props: { runner: Runner | null; state: string | null; version: Stat<string> | null }): ReactElement {
  throw new Error("not implemented");
}
