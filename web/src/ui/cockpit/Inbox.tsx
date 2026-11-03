import type { ReactElement } from "react";
import type { CockpitAction, InboxModel } from "../../protocol/cockpit.ts";

export function Inbox(props: {
  inbox: InboxModel | null;
  onAction(action: CockpitAction): void;
  onSelectSession(sessionId: string): void;
}): ReactElement {
  void props;
  throw new Error("not implemented");
}
