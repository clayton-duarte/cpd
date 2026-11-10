import type { ReactNode } from "react";

export interface MentionLinkProps {
  readonly agentKey: string;
  readonly href: string;
  readonly children: ReactNode;
}

export function linkifyMentions(
  text: string,
  liveAgentKeys: ReadonlySet<string>,
): ReactNode {
  throw new Error("stub: linkifyMentions not implemented");
}
