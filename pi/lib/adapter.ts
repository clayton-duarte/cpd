import { draftJobs, draftReason, tooLong } from "./askguard.ts";
import { toCpdError } from "./cpd.ts";
import { registerAgentState } from "./agent-state.ts";
import { createHost, type Host, type Options } from "./host.ts";
import { blockingReason, reasonText } from "./noblock.ts";
import type { PiApi } from "./pi.ts";
import { pnVerdict } from "./pnguard.ts";
import { RENDERED_TYPES } from "./render.ts";
import type { Run } from "./run.ts";
import { registerReloadCommand } from "./reload-command.ts";
import { start, stop, trackers } from "./startup.ts";
import { reasonText as testReasonText, unfilteredTestRun } from "./testguard.ts";
import { registerReminderCheck } from "./turns.ts";

export type { Options };
export { QUESTION_LABEL } from "./review.ts";

/** commands.md §15: wires the concerns to pi. Start-up and shutdown: `startup.ts`; the follower's Events: `handle.ts`; the Plan review: `review.ts`; the agent state: `agent-state.ts`; Context updates and the reminder: `turns.ts`; the reload command: `reload-command.ts`; a Manager's refused waits: `noblock.ts`; a Worker's refused full test runs: `testguard.ts`; refused `ask_user_question` calls: `askguard.ts`. */
export function adapter(pi: PiApi, options: Options = {}): void {
  const host = createHost(pi, options);

  // commands.md §15.9: one renderer for every cpd customType; its style is read at `session_start` (startup.ts).
  for (const type of RENDERED_TYPES) pi.registerMessageRenderer(type, host.rendering.render);
  registerReloadCommand(host);
  registerReminderCheck(host);

  pi.on("session_start", async (event, ctx) => {
    try {
      await start(host, ctx, typeof event?.reason === "string" ? event.reason : "unknown");
    } catch (e) {
      host.deliver("start-up", e);
    }
  });

  pi.on("session_shutdown", async () => {
    try {
      await stop(host);
    } catch (e) {
      host.deliver("shutdown", e);
    }
  });

  // §15.11, §15.12: one handler for the guards, registered once (pi has no unsubscribe); the role is read at each call.
  pi.on("tool_call", async (event) => {
    const role = host.run?.info.role;
    const pn = pnVerdict({ env: host.env, live: host.run !== undefined, store: globalThis as Record<string, unknown> }, event);
    if (pn) return pn;
    if (host.run && event.toolName === "ask_user_question") return askVerdict(host, host.run, event.input);
    if ((role !== "manager" && role !== "worker") || event.toolName !== "bash") return undefined;
    const input = event.input as { command?: unknown; timeout?: unknown };
    const command = typeof input.command === "string" ? input.command : "";
    if (role === "manager") {
      const matched = blockingReason(command, typeof input.timeout === "number" ? input.timeout : undefined);
      return matched === undefined ? undefined : { block: true, reason: reasonText(matched) };
    }
    const matched = unfilteredTestRun(command);
    return matched === undefined ? undefined : { block: true, reason: testReasonText(matched) };
  });

  // commands.md §15 "Every plannotator session is tracked": a plannotator result may have opened or closed a review.
  pi.on("tool_result", async (event) => {
    if (event.toolName !== "plannotator" && event.toolName !== "plannotator_submit_plan") return;
    const r = host.run;
    const tracker = r && trackers.get(r);
    if (tracker) await tracker.poll().catch((e) => host.deliver("plannotator tracker", e));
  });

  registerAgentState(host);
}

/** §15.13: G2 (any Session) before G1 (a Lead with a Plan: one `plan show`, best effort, a failed read refuses nothing). */
async function askVerdict(host: Host, r: Run, input: unknown): Promise<{ block: true; reason: string } | undefined> {
  const long = tooLong(input);
  if (long !== undefined) return { block: true, reason: long };
  if (r.info.role !== "lead" || r.info.plan === null) return undefined;
  try {
    const drafts = draftJobs(await host.call(["plan", "show", "--plan", r.info.plan]));
    return drafts > 0 ? { block: true, reason: draftReason(drafts) } : undefined;
  } catch (e) {
    r.log(`ask guard: plan show failed: ${toCpdError(e).message}`);
    return undefined;
  }
}
