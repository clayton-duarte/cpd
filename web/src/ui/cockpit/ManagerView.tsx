import type { ReactElement } from "react";
import type { ContextNode } from "../../protocol/cockpit.ts";

export interface ManagerViewProps {
  readonly context: ContextNode | null;
  readonly onSelectPlan: (planId: string) => void;
  readonly onSelectSession: (sessionId: string) => void;
}

// Stub: the implementation Job replaces this.
export function ManagerView(_props: ManagerViewProps): ReactElement | null {
  return null;
}
