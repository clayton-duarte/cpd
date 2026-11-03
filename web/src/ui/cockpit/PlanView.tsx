import type { ReactElement } from "react";
import type { CockpitAction, PlanViewModel } from "../../protocol/cockpit.ts";

export function PlanView(props: { plan: PlanViewModel | null; onAction(action: CockpitAction): void }): ReactElement {
  void props;
  throw new Error("not implemented");
}
