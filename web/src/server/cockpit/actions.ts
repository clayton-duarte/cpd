import { parseCockpitAction, type ActionOutput, type CockpitAction, type SnoozeDuration } from "../../protocol/cockpit.ts";
import { WebError } from "../../protocol/domain.ts";
import type { Cpd } from "./cli.ts";

export function actionArgs(action: CockpitAction): readonly string[] {
  switch (action.kind) {
    case "inbox.snooze":
      return ["inbox", "snooze", String(action.itemId), action.duration satisfies SnoozeDuration];
    case "inbox.dismiss": return ["inbox", "dismiss", String(action.itemId)];
    case "inbox.claim": return ["inbox", "claim", String(action.itemId)];
    case "inbox.approve": return ["inbox", "approve", String(action.itemId)];
    case "pr.post": return ["pr", "post", "--workflow", action.workflow, "--plan", action.planId];
    case "pr.merge": throw new Error("pr.merge needs the stored pull request: runAction resolves it");
    case "engine.start": return ["watch", "--ensure"];
    case "session.retier": return ["session", "retier", action.sessionId, "--tier", action.tier];
    case "plan.reject": return ["plan", "decide", action.review, "--reject", "--plan", action.planId];
  }
}
// `cpd pr merge <owner>/<repo>#<n>`: the pull request is the Workflow's stored pull_request, listed by `pr poll --plan`.
async function mergeArgs(cpd: Cpd, planId: string, workflow: string): Promise<readonly string[]> {
  const polled = await cpd.json(["pr", "poll", "--plan", planId]);
  const list = polled["workflows"];
  const found = Array.isArray(list) ? list.find((w: unknown) => typeof w === "object" && w !== null && (w as Record<string, unknown>)["key"] === workflow) : undefined;
  const pull = (found as Record<string, unknown> | undefined)?.["pull_request"];
  const repo = (pull as Record<string, unknown> | null | undefined)?.["repo"];
  const number = (pull as Record<string, unknown> | null | undefined)?.["number"];
  if (typeof repo !== "string" || typeof number !== "number") {
    throw new WebError("cpd_malformed_json", `workflow ${workflow} has no stored pull request`, { planId, workflow });
  }
  return ["pr", "merge", `${repo}#${number}`];
}
export async function runAction(cpd: Cpd, raw: unknown): Promise<ActionOutput> {
  const action = parseCockpitAction(raw);
  return cpd.json(action.kind === "pr.merge" ? await mergeArgs(cpd, action.planId, action.workflow) : actionArgs(action));
}
export const WEB_UNBLOCKS = "User message from the web chat";
export function sessionSendArgs(sessionId: string, text: string, unblocks?: string): readonly string[] {
  return ["session", "send", sessionId, "--unblocks", unblocks ?? WEB_UNBLOCKS, "--", text];
}
export function sessionRetierArgs(sessionId: string, tier: string): readonly string[] {
  return actionArgs({ kind: "session.retier", sessionId, tier });
}
