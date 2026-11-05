import type { Urgency } from "../../../protocol/cockpit.ts";

export const linkKinds: readonly string[] = ["plan-review", "pr-ready", "pr-closed", "review-requested", "deploy"];
/** Kinds whose left click opens the row's session (a Lead for the Plan kinds: the read model sets sessionId). */
export const sessionKinds: readonly string[] = ["dialog", "unread", "loop-guard", "question", "plan-review", "drafts-parked", "pr-ready", "pr-closed"];

export const urgencyText: Readonly<Record<Urgency, string>> = {
  red: "text-status-blocked",
  yellow: "text-status-landing",
  blue: "text-status-focus",
  magenta: "text-status-decision",
  cyan: "text-status-active",
  white: "text-muted-foreground",
};

export function age(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}
