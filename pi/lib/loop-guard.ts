import { isObject } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { Run } from "./run.ts";

/** commands.md §15.15: the loop guard of a Lead or Manager. */

/** L3: the sliding window, in ms. */
export const WINDOW_MS = 600_000;
/** The pi session entry that keeps a trip across a reload (L4 (c), L7). */
export const ENTRY_TYPE = "cpd-loop-guard";
export const KEY = "loop-guard";
export const LABEL = "φ: loop guard";

const guarded = (r: Run) => r.info.role === "lead" || r.info.role === "manager";

/** L7: the last `cpd-loop-guard` entry of the session says whether the breaker is tripped. */
export function trippedIn(entries: readonly unknown[]): boolean {
  const last = entries.findLast((e) => isObject(e) && e.type === "custom" && e.customType === ENTRY_TYPE);
  return isObject(last) && isObject(last.data) && last.data.tripped === true;
}

/** Opens the reason and the gate, without reporting: the caller refreshes. */
export function holdTrip(r: Run): void {
  r.tripped = true;
  r.facts.open(KEY, LABEL, "human");
}

/** L5, L6: the window is emptied and the trip, when there is one, is cleared (the reason closes, the entry says so, deliveries trigger turns again). True when it was tripped. */
export function clearTrip(host: Host, r: Run): boolean {
  r.turnStarts = [];
  if (!r.tripped) return false;
  r.tripped = false;
  r.facts.close(KEY);
  host.pi.appendEntry(ENTRY_TYPE, { tripped: false });
  host.refresh(r);
  return true;
}

/** L4: the breaker trips. */
function trip(host: Host, r: Run, at: number): void {
  holdTrip(r);
  host.pi.appendEntry(ENTRY_TYPE, { tripped: true, at, turns: r.turnStarts.length });
  host.refresh(r);
}

/** L8: the fixed bounds of one turn. */
export const READ_STREAK_LIMIT = 12;
export const TURN_TOOL_LIMIT = 30;
export const TURN_REFUSAL_TRIP = 3;

const STATE_TOOLS = new Set(["write", "edit", "cpd_plan_review", "ask_user_question"]);
/** L8: the state-mutating `cpd` commands, `group -> subcommands`; the one list. */
const STATE_COMMANDS: Record<string, readonly string[]> = {
  job: ["add", "edit", "cancel", "retry", "steer", "addendum", "urgent", "approve-tests", "approve-touches"],
  workflow: ["add", "edit", "describe", "cancel", "start"],
  plan: ["new", "phase", "outline", "lock", "review", "rename", "move", "done", "cancel", "archive"],
  pr: ["open", "merge", "post", "rebase", "push", "describe", "reply", "rerun"],
  context: ["new", "decide", "note", "ask", "answer", "brief", "plan", "rename", "archive", "delete"],
  session: ["send", "close", "resume", "rename", "fork"],
  escalation: ["answer"],
  review: ["submit"],
  inbox: ["claim"],
  preset: ["apply"],
  board: ["post"],
};
const STATE_COMMAND = new RegExp(
  `(?<![A-Za-z0-9_.-])cpd\\s+(?:(?:${Object.entries(STATE_COMMANDS)
    .map(([g, subs]) => `${g}\\s+(?:${subs.join("|")})`)
    .join("|")})(?![A-Za-z0-9_-])|answer(?![A-Za-z0-9_-]))`,
);

/** L8: the MCP write tools (Atlassian, Slack); one source for a direct tool name and a `tools.<name>` in codemode. */
const MCP_WRITE_NAME = "(?:mcp__atlassian__(?:create|edit|transition|add|update|delete)[A-Za-z0-9_]*|mcp__slack__slack_(?:send|add|create|update)[A-Za-z0-9_]*)";
const MCP_WRITE_TOOL = new RegExp(`^${MCP_WRITE_NAME}$`);
const MCP_WRITE_CALL = new RegExp(`(?<![A-Za-z0-9_])tools\\.${MCP_WRITE_NAME}(?![A-Za-z0-9_])`);

/** L8: whether a tool call records state (resets the read streak and turn refusals); everything else is a read. */
export function isStateWrite(toolName: string, input: unknown): boolean {
  if (STATE_TOOLS.has(toolName) || MCP_WRITE_TOOL.test(toolName)) return true;
  const field = toolName === "bash" ? "command" : toolName === "codemode" ? "code" : undefined;
  if (field === undefined || !isObject(input)) return false;
  const text = input[field];
  return typeof text === "string" && (STATE_COMMAND.test(text) || (toolName === "codemode" && MCP_WRITE_CALL.test(text)));
}

const READ_STREAK_REASON =
  "φ: loop guard: this tool call was NOT executed: more than 12 consecutive read-only tool calls in one turn. Stop your turn now. Delegate codebase exploration to a scout Job (cpd job add <k> --kind scout), or run the cpd state command that records your decision.";
const TOTAL_CALLS_REASON =
  "φ: loop guard: this tool call was NOT executed: more than 30 read-only tool calls in one turn. Stop your turn now: end it with what you have, and delegate any further codebase exploration to a scout Job.";

/** L8: counts a call of the model; the refusal reason, or undefined to let it run. */
export function toolCallGuard(host: Host, call: { toolName: string; input: unknown; parentToolCallId?: string }): string | undefined {
  const r = host.run;
  if (!r || !guarded(r) || call.parentToolCallId !== undefined) return undefined;
  if (isStateWrite(call.toolName, call.input)) {
    r.readStreak = 0;
    r.turnRefusals = 0;
  } else {
    r.totalCalls += 1;
    r.readStreak += 1;
  }
  const reason = r.totalCalls > TURN_TOOL_LIMIT ? TOTAL_CALLS_REASON : r.readStreak > READ_STREAK_LIMIT ? READ_STREAK_REASON : undefined;
  if (reason === undefined) return undefined;
  r.turnRefusals += 1;
  if (r.turnRefusals >= TURN_REFUSAL_TRIP && !r.tripped) trip(host, r, (host.options.now ?? Date.now)());
  return reason;
}

export const resetTurn = (r: Run) => {
  r.readStreak = 0;
  r.totalCalls = 0;
  r.turnRefusals = 0;
};

/** Registered once (pi has no unsubscribe); the run and its role are read at each call. */
export function registerLoopGuard(host: Host): void {
  host.pi.on("before_agent_start", function loopGuardHandler(event) {
    const r = host.run;
    if (!r || !guarded(r)) return;
    resetTurn(r);
    // Only the User's prompt is `messageType: "user"` (not `startsReply`, which also counts a from-null session-message).
    if (event.messageType === "user") {
      clearTrip(host, r);
      return;
    }
    const now = (host.options.now ?? Date.now)();
    r.turnStarts = [...r.turnStarts.filter((t) => t > now - WINDOW_MS), now];
    if (!r.tripped && r.turnStarts.length > r.info.loopGuardTurns) trip(host, r, now);
  });
  host.pi.on("agent_end", function loopGuardAgentEnd() {
    const r = host.run;
    if (r && guarded(r)) resetTurn(r);
  });
}
