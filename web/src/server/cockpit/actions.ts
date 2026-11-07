import { parseCockpitAction, type ActionOutput, type CockpitAction, type SnoozeDuration } from "../../protocol/cockpit.ts";
import { WebError } from "../../protocol/domain.ts";
import { toArgv, type CpdCommand } from "../cpd-argv.ts";
import type { Cpd } from "./cli.ts";

export function actionArgs(action: CockpitAction): CpdCommand {
  switch (action.kind) {
    case "inbox.snooze": return { kind: "inbox.snooze", itemId: action.itemId, duration: action.duration satisfies SnoozeDuration };
    case "inbox.dismiss": return { kind: "inbox.dismiss", itemId: action.itemId };
    case "inbox.claim": return { kind: "inbox.claim", itemId: action.itemId };
    case "inbox.approve": return { kind: "inbox.approve", itemId: action.itemId };
    case "pr.post":
    case "pr.repost": {
      if (action.workflow === undefined || action.workflow === "") throw new Error(`${action.kind} needs a workflow: runAction resolves it`);
      return { kind: "pr.post", verb: action.kind === "pr.post" ? "post" : "repost", workflow: action.workflow, planId: action.planId };
    }
    case "pr.merge": {
      if (action.pull === undefined || action.pull === "") throw new Error("pr.merge needs the stored pull request: runAction resolves it");
      return { kind: "pr.merge", pull: action.pull };
    }
    case "error.report":
      // a Manager's key is the bare Context id (cpd, host; agent keys are `<role> <context>[:<plan>[:<job>]]`),
      // so `cpd session send cpd ...`, never `cpd:manager` (air-n12).
      return { kind: "session.send", sessionId: action.target, unblocks: `WebUI error toast (${action.code})`, text: `Error toast reported from WebUI:\n- code: ${action.code}\n- message: ${action.message}` };
    case "engine.start": return { kind: "watch.ensure" };
    case "context.retire":
      return { kind: "context.retire", contextId: action.contextId, approve: action.approve, reject: action.reject };
    case "session.retier": return { kind: "session.retier", sessionId: action.sessionId, tier: action.tier };
    case "plan.reject": return { kind: "plan.decide", review: action.review, decision: "reject", planId: action.planId };
    case "plan.new": return { kind: "plan.new", planId: action.planId, contextId: action.contextId, title: action.title };
    case "context.rename": return { kind: "context.rename", contextId: action.contextId, title: action.title };
    case "plan.rename": return { kind: "plan.rename", planId: action.planId, title: action.title };
    case "session.rename": return { kind: "session.rename", sessionId: action.sessionId, name: action.name };
    case "session.fork": return { kind: "session.fork", sourceId: action.sourceId, name: action.name };
    case "session.close": return { kind: "session.close", sessionId: action.sessionId };
    case "context.archive": return { kind: "context.archive", contextId: action.contextId };
    case "plan.archive": return { kind: "plan.archive", planId: action.planId };
    case "session.archive": return { kind: "session.archive", sessionId: action.sessionId };
    case "context.unarchive": return { kind: "context.unarchive", contextId: action.contextId };
    case "plan.unarchive": return { kind: "plan.unarchive", planId: action.planId };
    case "session.unarchive": return { kind: "session.unarchive", sessionId: action.sessionId };
    case "context.delete": return { kind: "context.delete", contextId: action.contextId };
    case "plan.delete": return { kind: "plan.delete", planId: action.planId };
  }
}
// `cpd pr merge <owner>/<repo>#<n>`: the pull request is the Workflow's stored pull_request, listed by `pr poll --plan`.
async function mergeArgs(cpd: Cpd, planId: string, workflow: string): Promise<CpdCommand> {
  const found = (await polledWorkflows(cpd, planId)).find((w) => w["key"] === workflow);
  const pull = (found as Record<string, unknown> | undefined)?.["pull_request"];
  const repo = (pull as Record<string, unknown> | null | undefined)?.["repo"];
  const number = (pull as Record<string, unknown> | null | undefined)?.["number"];
  if (typeof repo !== "string" || typeof number !== "number") {
    throw new WebError("cpd_malformed_json", `workflow ${workflow} has no stored pull request`, { planId, workflow });
  }
  return { kind: "pr.merge", pull: `${repo}#${number}` };
}
async function polledWorkflows(cpd: Cpd, planId: string): Promise<Record<string, unknown>[]> {
  const list = (await cpd.json(toArgv({ kind: "plan.show", planId })))["workflows"];
  return Array.isArray(list) ? list.filter((w): w is Record<string, unknown> => typeof w === "object" && w !== null) : [];
}
// post and repost take the Workflow key: find the one whose stored pull request is `<owner>/<repo>#<n>`.
async function prVerbArgs(cpd: Cpd, verb: "post" | "repost", planId: string, pull: string | undefined): Promise<CpdCommand> {
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
  return { kind: "pr.post", verb, workflow: key, planId };
}
const NEEDS_SLOT: ReadonlySet<string> = new Set(["context.archive", "plan.archive", "session.archive", "session.close"]);
async function resolveArgs(cpd: Cpd, action: CockpitAction): Promise<CpdCommand> {
  const has = (v: string | undefined): v is string => v !== undefined && v !== "";
  if (action.kind === "pr.merge" && !has(action.pull)) return mergeArgs(cpd, action.planId ?? "", action.workflow ?? "");
  if ((action.kind === "pr.post" || action.kind === "pr.repost") && !has(action.workflow)) {
    return prVerbArgs(cpd, action.kind === "pr.post" ? "post" : "repost", action.planId, action.pull);
  }
  return actionArgs(action);
}
export async function runAction(cpd: Cpd, raw: unknown): Promise<ActionOutput> {
  const action = parseCockpitAction(raw);
  const args = toArgv(await resolveArgs(cpd, action));
  try {
    return await cpd.json(args);
  } catch (e) {
    // the chat slot must be free: park once, then retry once
    if (NEEDS_SLOT.has(action.kind) && e instanceof WebError && e.details?.["cpdCode"] === "in_chat_slot") {
      // a failed park is not the answer: the retry reports the real error
      await cpd.json(toArgv({ kind: "session.park" })).catch(() => undefined);
      return cpd.json(args);
    }
    throw e;
  }
}
export const WEB_UNBLOCKS = "User message from the web chat";
export function sessionSendArgs(sessionId: string, text: string, unblocks?: string): readonly string[] {
  return toArgv({ kind: "session.send", sessionId, text, unblocks: unblocks ?? WEB_UNBLOCKS });
}
export function sessionRetierArgs(sessionId: string, tier: string): readonly string[] {
  return toArgv(actionArgs({ kind: "session.retier", sessionId, tier }));
}
