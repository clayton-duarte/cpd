import { z } from "zod";
import { CpdError, usage } from "../lib/errors.ts";
import { assertNoControlChars, herdr } from "./herdr.ts";
import { AgentSession } from "./panes.ts";

/** herdr's own rule for agent names (the contract's `c<12 hex>` names meet it). */
const AGENT_NAME = /^[a-z][a-z0-9_-]{0,31}$/;
/** herdr 0.9.3 accepts `agent start --timeout` only above 3000 ms and up to 300000 ms (probed; see the Attempt report). */
const MIN_START_TIMEOUT_MS = 3001;
const MAX_START_TIMEOUT_MS = 300_000;
/** Backstop beyond herdr's own `--timeout`, so a caller timeout is reported by herdr rather than killed. */
const BACKSTOP_MS = 15_000;

const Agent = z.looseObject({
  name: z.string(),
  pane_id: z.string(),
  agent_status: z.string(),
  agent_session: AgentSession.optional(),
});
export type Agent = z.infer<typeof Agent>;

const Started = z.looseObject({
  agent: z.looseObject({ agent_session: AgentSession }),
  argv: z.array(z.string()),
});
const AgentInfo = z.looseObject({ agent: Agent });

export function assertAgentName(name: string): void {
  if (!AGENT_NAME.test(name)) {
    throw usage("invalid_agent_name", `invalid herdr agent name: ${JSON.stringify(name)} (must match ${AGENT_NAME.source})`, { name });
  }
}

/**
 * Lead-only tools, denied to every session cpd starts (Workers, Reviewers). The Lead set is installed globally, so
 * these would otherwise load there too. Sources (inspected 2026-10-13):
 * - `ask_user_question`: @juicesharp/rpiv-ask-user-question@2.13.8;
 * - `cpd_plan_review`: cpd's pi adapter (`pi/extension.ts`);
 * - `plannotator`, `plannotator_mark_done`, `plannotator_submit_plan`, `plannotator_inbox`: @plannotator/pi-extension@0.28.17.
 */
export const LEAD_ONLY_TOOLS = [
  "ask_user_question",
  "cpd_plan_review",
  "plannotator",
  "plannotator_mark_done",
  "plannotator_submit_plan",
  "plannotator_inbox",
] as const;

/** Flags that strip Lead powers: no skills (the `cpd-lead` skill) and no Lead-only tools. */
const notLeadArgs = (): string[] => ["--no-skills", "--exclude-tools", LEAD_ONLY_TOOLS.join(",")];

/** Pure: pi arguments for a Worker. `modelArg` is `<provider/id>:<level>`. `--approve` trusts project-local `.pi/` (t1 F10). */
export const workerArgs = (modelArg: string): string[] => ["--approve", ...notLeadArgs(), "--model", modelArg];

/**
 * Pure: pi arguments for a Reviewer: read-only tools, forked from the Lead session.
 * `null` (reviewMode "fresh") drops `--fork`: a new read-only session with no Lead conversation.
 */
export const reviewerArgs = (leadSessionPath: string | null, modelArg: string): string[] => [
  "--approve",
  ...notLeadArgs(),
  ...(leadSessionPath === null ? [] : ["--fork", leadSessionPath]),
  "--tools",
  "read,grep,find,ls",
  "--model",
  modelArg,
];

export type StartAgent = { name: string; kind: "pi"; pane: string; timeoutMs: number; args: string[] };

/**
 * `herdr agent start <name> --kind pi --pane <pane> --timeout <ms> -- <args>`.
 * Returns the argv herdr typed (`ps` is useless for pi, t1 F2) and the session path herdr reports.
 * The session file may not exist yet (t1 F10).
 */
export async function startAgent(o: StartAgent): Promise<{ argv: string[]; sessionPath: string }> {
  assertAgentName(o.name);
  if (!Number.isInteger(o.timeoutMs) || o.timeoutMs < MIN_START_TIMEOUT_MS || o.timeoutMs > MAX_START_TIMEOUT_MS) {
    throw usage("invalid_timeout", `agent start timeout must be an integer ${MIN_START_TIMEOUT_MS}-${MAX_START_TIMEOUT_MS} ms, got ${String(o.timeoutMs)}`, { timeoutMs: o.timeoutMs });
  }
  assertNoControlChars("pane id", o.pane);
  o.args.forEach((a, i) => assertNoControlChars(`agent argument ${i}`, a));
  const r = await herdr(
    ["agent", "start", o.name, "--kind", o.kind, "--pane", o.pane, "--timeout", String(o.timeoutMs), "--", ...o.args],
    Started,
    { timeoutMs: o.timeoutMs + BACKSTOP_MS },
  );
  return { argv: r.argv, sessionPath: r.agent.agent_session.value };
}

/**
 * `herdr agent prompt <name> <text> --wait [--timeout <ms>]`. Always waits, never a bare `agent wait` (t1 F7).
 * Success only means the turn ended: callers must check the artefact (session `stopReason`, submission).
 */
export async function promptAgent(name: string, text: string, o: { wait: true; timeoutMs?: number }): Promise<Agent> {
  assertAgentName(name);
  const timeout = o.timeoutMs === undefined ? [] : ["--timeout", String(o.timeoutMs)];
  const r = await herdr(["agent", "prompt", name, text, "--wait", ...timeout], AgentInfo, {
    timeoutMs: o.timeoutMs === undefined ? null : o.timeoutMs + BACKSTOP_MS,
  });
  return r.agent;
}

/** `herdr agent get`; null when herdr says `agent_not_found` (the agent is gone). */
export async function agentGet(name: string): Promise<Agent | null> {
  assertAgentName(name);
  try {
    return (await herdr(["agent", "get", name], AgentInfo)).agent;
  } catch (e) {
    if (e instanceof CpdError && e.code === "herdr_agent_not_found") return null;
    throw e;
  }
}
