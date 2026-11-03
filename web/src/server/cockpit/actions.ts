import type { ActionOutput, CockpitAction, SnoozeDuration } from "../../protocol/cockpit.ts";
import { WebError } from "../../protocol/domain.ts";
import type { Cpd } from "./cli.ts";

const DURATIONS: readonly string[] = ["15m", "1h", "tomorrow"];

function field(o: Record<string, unknown>, name: string, kind: string): void {
  const v = o[name];
  const ok = name === "itemId" ? Number.isInteger(v) && (v as number) >= 0
    : name === "duration" ? typeof v === "string" && DURATIONS.includes(v)
    : typeof v === "string" && v !== "";
  if (!ok) throw new WebError("invalid_message", `action ${kind}: bad field ${name}`, { field: name });
}

export function parseCockpitAction(raw: unknown): CockpitAction {
  if (typeof raw !== "object" || raw === null) throw new WebError("unknown_action", "action is not an object");
  const o = raw as Record<string, unknown>;
  const kind = o["kind"];
  const fields: Record<string, readonly string[]> = {
    "inbox.snooze": ["itemId", "duration"],
    "inbox.dismiss": ["itemId"],
    "inbox.claim": ["itemId"],
    "inbox.approve": ["itemId"],
    "pr.post": ["planId", "workflow"],
    "session.retier": ["sessionId", "tier"],
    "plan.reject": ["planId", "review"],
  };
  if (typeof kind !== "string" || !Object.hasOwn(fields, kind)) throw new WebError("unknown_action", `unknown action kind ${String(kind)}`);
  for (const f of fields[kind]!) field(o, f, kind);
  return o as unknown as CockpitAction;
}

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
