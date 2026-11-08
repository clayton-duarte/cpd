import { PROTOCOL_NAME as ASK_PROTOCOL, tooLong, unseen } from "./askguard.ts";
import { registerAgentState } from "./agent-state.ts";
import { registerCompactionRatio } from "./compaction-ratio.ts";
import { createHost, type Options } from "./host.ts";
import { registerLoopGuard, toolCallGuard } from "./loop-guard.ts";
import { noteQuestionTool, registerQuestionGuard } from "./question-guard.ts";
import { dirname, join } from "node:path";
import { registerLiveStream } from "./live-stream.ts";
import { homedir } from "node:os";
import { exploreGuard, leadExploreGuard } from "./explore.ts";
import { fileURLToPath } from "node:url";
import { pinAdapter } from "./adapter-pin.ts";
import { PROTOCOL_NAME as MAIN_PROTOCOL, primaryCheckoutMove, primaryCheckoutWrite, reasonText as mainReasonText } from "./mainguard.ts";
import { BOUND_SECONDS, blockingReason, reasonText } from "./noblock.ts";
import type { PiApi } from "./pi.ts";
import { installHint } from "./hint.ts";
import { installQuitMarker } from "./quit.ts";
import { installChatPad, registerChatPad } from "./pad.ts";
import { PROTOCOL_NAME as SECRET_PROTOCOL, reasonText as secretReasonText, secretBypass } from "./secretguard.ts";
import { electronBypass, reasonText as electronReasonText } from "./electronguard.ts";
import { PROTOCOL_NAME as EXTERNAL_PROTOCOL, externalBlock } from "./externalguard.ts";
import { hostApproveBypass, reasonText as hostApproveReasonText } from "./hostapprove.ts";
import { RENDERED_TYPES } from "./render.ts";
import { registerReloadCommand } from "./reload-command.ts";
import { start, stop } from "./startup.ts";
import { reasonText as testReasonText, unfilteredTestRun } from "./testguard.ts";
import { registerReminderCheck } from "./turns.ts";

export type { Options };
export { QUESTION_LABEL } from "./review.ts";

/** commands.md §15: wires the concerns to pi. Start-up and shutdown: `startup.ts`; the follower's Events: `handle.ts`; the Plan review: `review.ts`; the agent state: `agent-state.ts`; Context updates and the reminder: `turns.ts`; the reload command: `reload-command.ts`; a Manager's and a Lead's refused waits: `noblock.ts`; a Worker's refused full test runs: `testguard.ts`; a primary checkout's refused git commands, bash writes and edit/write calls: `mainguard.ts`; refused `ask_user_question` calls: `askguard.ts`. */
export function adapter(pi: PiApi, options: Options = {}): void {
  registerCompactionRatio(); // §15.19: pi's SettingsManager derives the reserve from the compaction ratio
  registerChatPad(); // §15.9: hook the fullscreen root before pi's first mount, so the pad catches it too
  const host = createHost(pi, options);
  try {
    pinAdapter(join(dirname(fileURLToPath(import.meta.url)), ".."));
  } catch (e) {
    host.deliver("adapter pin", e); // a pin that cannot be written must not stop pi
  }

  // commands.md §15.9: one renderer for every cpd customType; its style is read at `session_start` (startup.ts).
  for (const type of RENDERED_TYPES) pi.registerMessageRenderer(type, host.rendering.render);
  registerReloadCommand(host);
  registerReminderCheck(host);
  registerLoopGuard(host);
  registerQuestionGuard(host);

  registerLiveStream(pi, host.env); // contract §6.21

  let removeHint: (() => void) | undefined;
  pi.on("session_start", async (event, ctx) => {
    removeHint ??= installHint(); // §6.1.1: before pi first draws the jump hint
    // contract §6.1: the uniform 1-cell chat-slot pad replaces the cpd-pad status and header
    if (ctx.mode === "tui") installChatPad(ctx);
    if (ctx.mode === "tui") installQuitMarker(ctx as never, host);
    try {
      await start(host, ctx, typeof event?.reason === "string" ? event.reason : "unknown");
    } catch (e) {
      host.deliver("start-up", e);
    }
  });

  pi.on("session_shutdown", async () => {
    removeHint?.();
    removeHint = undefined;
    try {
      await stop(host);
    } catch (e) {
      host.deliver("shutdown", e);
    }
  });

  // §15.11, §15.12, §15.16: one handler for the guards, registered once (pi has no unsubscribe); the role is read at each call.
  pi.on("tool_call", async (event, ctx) => {
    noteQuestionTool(host, event.toolName);
    const looped = toolCallGuard(host, { toolName: event.toolName, input: event.input, parentToolCallId: event.parentToolCallId });
    if (looped !== undefined) return { block: true, reason: looped };
    // commands.md §10.18, contract §5.2: a guard's refusal also records one `protocol-breach` Event in the Ledger,
    // with the protocol it enforces, the guard, the refused tool and the refused command line (none for a question).
    const breach = (protocol: string, source: string, tool: string, command: string | undefined, reason: string): void => {
      const id = host.run?.id ?? host.env.CPD_SESSION;
      const args = ["session", "report", "--kind", "protocol-breach", "--protocol", protocol, "--source", source, "--tool", tool, "--message", reason, ...(command === undefined ? [] : ["--command", command]), ...(id === undefined ? [] : ["--session", id])];
      host.call(args).catch((e) => host.deliver("protocol breach", e)); // L4: a failed call is logged, never retried
    };
    const role = host.run?.info.role;
    const external = host.run && externalBlock(event.toolName, event.input as Record<string, unknown>, { grantable: role !== "worker" });
    if (external) {
      const command = typeof (event.input as { command?: unknown }).command === "string" ? (event.input as { command: string }).command : undefined;
      breach(EXTERNAL_PROTOCOL, "externalguard", event.toolName, command, external);
      return { block: true, reason: external };
    }
    const explored = exploreGuard(role, host.env, { name: event.toolName, arguments: event.input as Record<string, unknown> }, ctx.cwd);
    if (explored !== undefined) return { block: true, reason: explored.block };
    const led = leadExploreGuard(role, host.env, { name: event.toolName, arguments: event.input as Record<string, unknown> });
    if (led !== undefined) return { block: true, reason: led.block };
    if (host.run && (event.toolName === "edit" || event.toolName === "write")) {
      const written = primaryCheckoutWrite(event.toolName, event.input, ctx.cwd, homedir());
      if (written !== undefined) {
        const reason = mainReasonText(written);
        breach(MAIN_PROTOCOL, "mainguard", event.toolName, undefined, reason);
        return { block: true, reason };
      }
    }
    const bashCommand = (event.input as { command?: unknown }).command;
    if (host.run && event.toolName === "bash" && typeof bashCommand === "string") {
      const moved = primaryCheckoutMove(bashCommand, ctx.cwd, homedir());
      if (moved !== undefined) {
        const reason = mainReasonText(moved);
        breach(MAIN_PROTOCOL, "mainguard", "bash", bashCommand, reason);
        return { block: true, reason };
      }
      const secret = secretBypass(bashCommand);
      if (secret !== undefined) {
        const reason = secretReasonText(secret);
        breach(SECRET_PROTOCOL, "secretguard", "bash", bashCommand, reason);
        return { block: true, reason };
      }
      const electron = electronBypass(bashCommand); if (electron !== undefined) return { block: true, reason: electronReasonText(electron) };
      const hostApprove = hostApproveBypass(bashCommand); if (hostApprove !== undefined) return { block: true, reason: hostApproveReasonText(hostApprove) };
    }
    if (host.run && event.toolName === "ask_user_question") {
      const long = tooLong(event.input);
      if (long !== undefined) {
        breach(ASK_PROTOCOL, "askguard", "ask_user_question", undefined, long);
        return { block: true, reason: long };
      }
      const unseenReason = role === "lead" || role === "manager" ? unseen(event.input, event.parentToolCallId ?? event.toolCallId, ctx.sessionManager.getBranch()) : undefined;
      if (unseenReason !== undefined) {
        breach(ASK_PROTOCOL, "askguard", "ask_user_question", undefined, unseenReason);
        return { block: true, reason: unseenReason };
      }
      host.run.facts.hold(event.input); // §15.4: not refused, so a Verdict option names the dialog it opens
      return undefined;
    }
    if ((role !== "manager" && role !== "lead" && role !== "worker") || event.toolName !== "bash") return undefined;
    const input = event.input as { command?: unknown; timeout?: unknown };
    const command = typeof input.command === "string" ? input.command : "";
    if (role === "manager" || role === "lead") {
      const matched = blockingReason(command, typeof input.timeout === "number" ? input.timeout : undefined);
      if (matched !== undefined) return { block: true, reason: reasonText(matched, role) };
      if (input.timeout === undefined) input.timeout = BOUND_SECONDS; // pro-d202: pi's bash tool has no default timeout
      return undefined;
    }
    const matched = unfilteredTestRun(command);
    return matched === undefined ? undefined : { block: true, reason: testReasonText(matched) };
  });

  registerAgentState(host);
}
