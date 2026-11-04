import { parseCockpitAction, type ActionOutput, type CockpitAction, type SnoozeDuration } from "../../protocol/cockpit.ts";
import { WebError } from "../../protocol/domain.ts";
import type { Cpd } from "./cli.ts";

export function actionArgs(action: CockpitAction): readonly string[] {
  switch (action.kind) {
    case "inbox.snooze": return ["inbox", "snooze", String(action.itemId), action.duration satisfies SnoozeDuration];
    case "inbox.dismiss": return ["inbox", "dismiss", String(action.itemId)];
    case "inbox.claim": return ["inbox", "claim", String(action.itemId)];
    case "inbox.approve": return ["inbox", "approve", String(action.itemId)];
    case "pr.post": return ["pr", "post", "--workflow", action.workflow, "--plan", action.planId];
    case "session.retier": return ["session", "retier", action.sessionId, "--tier", action.tier];
    case "plan.reject": return ["plan", "decide", action.review, "--reject", "--plan", action.planId];
  }
}
export async function runAction(cpd: Cpd, raw: unknown): Promise<ActionOutput> {
  return cpd.json(actionArgs(parseCockpitAction(raw)));
}
export const WEB_UNBLOCKS = "User message from the web chat";
export function sessionSendArgs(sessionId: string, text: string, unblocks?: string): readonly string[] {
  return ["session", "send", sessionId, "--unblocks", unblocks ?? WEB_UNBLOCKS, "--", text];
}
export function sessionRetierArgs(sessionId: string, tier: string): readonly string[] {
  return actionArgs({ kind: "session.retier", sessionId, tier });
}
