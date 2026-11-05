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
    case "pr.post":
    case "pr.repost": {
      if (action.workflow === undefined || action.workflow === "") throw new Error(`${action.kind} needs a workflow: runAction resolves it`);
      return ["pr", action.kind === "pr.post" ? "post" : "repost", "--workflow", action.workflow, "--plan", action.planId];
    }
    case "pr.merge": {
      if (action.pull === undefined || action.pull === "") throw new Error("pr.merge needs the stored pull request: runAction resolves it");
      return ["pr", "merge", action.pull];
    }
    case "engine.start": return ["watch", "--ensure"];
    case "context.retire": {
      const args = ["context", "retire", `--context=${action.contextId}`];
      if (typeof action.approve === "string" && action.approve !== "") args.push(`--approve=${action.approve}`);
      if (typeof action.reject === "string" && action.reject !== "") args.push(`--reject=${action.reject}`);
      return args;
    }
    case "session.retier": return ["session", "retier", action.sessionId, "--tier", action.tier];
    case "plan.reject": return ["plan", "decide", action.review, "--reject", "--plan", action.planId];
    case "plan.new": return ["plan", "new", action.planId, `--context=${action.contextId}`, `--title=${action.title}`, "--by-user"];
    case "context.rename": return ["context", "rename", action.contextId, `--title=${action.title}`];
    case "plan.rename": return ["plan", "rename", `--plan=${action.planId}`, `--title=${action.title}`];
    case "session.rename": return ["session", "rename", `--session=${action.sessionId}`, "--", action.name];
    case "session.fork": return ["session", "fork", action.sourceId, `--name=${action.name}`];
    case "session.close": return ["session", "close", action.sessionId];
    case "context.archive": return ["context", "archive", action.contextId];
    case "plan.archive": return ["plan", "archive", `--plan=${action.planId}`];
    case "session.archive": return ["session", "archive", action.sessionId];
    case "context.unarchive": return ["context", "unarchive", action.contextId];
    case "plan.unarchive": return ["plan", "unarchive", `--plan=${action.planId}`];
    case "session.unarchive": return ["session", "unarchive", action.sessionId];
    case "context.delete": return ["context", "delete", action.contextId, "--yes"];
    case "plan.delete": return ["plan", "delete", action.planId, "--yes"];
  }
}
// `cpd pr merge <owner>/<repo>#<n>`: the pull request is the Workflow's stored pull_request, listed by `pr poll --plan`.
async function mergeArgs(cpd: Cpd, planId: string, workflow: string): Promise<readonly string[]> {
  const found = (await polledWorkflows(cpd, planId)).find((w) => w["key"] === workflow);
  const pull = (found as Record<string, unknown> | undefined)?.["pull_request"];
  const repo = (pull as Record<string, unknown> | null | undefined)?.["repo"];
  const number = (pull as Record<string, unknown> | null | undefined)?.["number"];
  if (typeof repo !== "string" || typeof number !== "number") {
    throw new WebError("cpd_malformed_json", `workflow ${workflow} has no stored pull request`, { planId, workflow });
  }
  return ["pr", "merge", `${repo}#${number}`];
}
async function polledWorkflows(cpd: Cpd, planId: string): Promise<Record<string, unknown>[]> {
  const list = (await cpd.json(["pr", "poll", "--plan", planId]))["workflows"];
  return Array.isArray(list) ? list.filter((w): w is Record<string, unknown> => typeof w === "object" && w !== null) : [];
}
// post and repost take the Workflow key: find the one whose stored pull request is `<owner>/<repo>#<n>`.
async function prVerbArgs(cpd: Cpd, verb: "post" | "repost", planId: string, pull: string | undefined): Promise<readonly string[]> {
  const hash = pull === undefined ? -1 : pull.indexOf("#");
  const repo = pull === undefined || hash < 0 ? "" : pull.slice(0, hash);
  const number = pull === undefined || hash < 0 ? NaN : Number(pull.slice(hash + 1));
  const found = (await polledWorkflows(cpd, planId)).find((w) => {
    const p = w["pull_request"];
    return typeof p === "object" && p !== null && (p as Record<string, unknown>)["repo"] === repo && (p as Record<string, unknown>)["number"] === number;
  });
  const key = found?.["key"];
  if (typeof key !== "string" || key === "") {
    throw new WebError("cpd_malformed_json", `plan ${planId} has no workflow for pull request ${String(pull)}`, { planId, pull });
  }
  return ["pr", verb, "--workflow", key, "--plan", planId];
}
const NEEDS_SLOT: ReadonlySet<string> = new Set(["context.archive", "plan.archive", "session.archive", "session.close"]);
async function resolveArgs(cpd: Cpd, action: CockpitAction): Promise<readonly string[]> {
  const has = (v: string | undefined): v is string => v !== undefined && v !== "";
  if (action.kind === "pr.merge" && !has(action.pull)) return mergeArgs(cpd, action.planId ?? "", action.workflow ?? "");
  if ((action.kind === "pr.post" || action.kind === "pr.repost") && !has(action.workflow)) {
    return prVerbArgs(cpd, action.kind === "pr.post" ? "post" : "repost", action.planId, action.pull);
  }
  return actionArgs(action);
}
export async function runAction(cpd: Cpd, raw: unknown): Promise<ActionOutput> {
  const action = parseCockpitAction(raw);
  const args = await resolveArgs(cpd, action);
  try {
    return await cpd.json(args);
  } catch (e) {
    // the chat slot must be free: park once, then retry once
    if (NEEDS_SLOT.has(action.kind) && e instanceof WebError && e.details?.["cpdCode"] === "in_chat_slot") {
      // a failed park is not the answer: the retry reports the real error
      await cpd.json(["session", "park"]).catch(() => undefined);
      return cpd.json(args);
    }
    throw e;
  }
}
export const WEB_UNBLOCKS = "User message from the web chat";
export function sessionSendArgs(sessionId: string, text: string, unblocks?: string): readonly string[] {
  return ["session", "send", sessionId, "--unblocks", unblocks ?? WEB_UNBLOCKS, "--", text];
}
export function sessionRetierArgs(sessionId: string, tier: string): readonly string[] {
  return actionArgs({ kind: "session.retier", sessionId, tier });
}
