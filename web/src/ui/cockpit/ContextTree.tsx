import type { ReactElement } from "react";
import type { ContextTreeModel } from "../../protocol/cockpit.ts";

export function ContextTree(props: {
  tree: ContextTreeModel | null;
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
}): ReactElement {
  void props;
  throw new Error("not implemented");
}
