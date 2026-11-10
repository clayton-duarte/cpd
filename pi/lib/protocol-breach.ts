import { homedir } from "node:os";
import { PROTOCOL_NAME as ASK_PROTOCOL, tooLong, unseen } from "./askguard.ts";
import { PROTOCOL_NAME as EXTERNAL_PROTOCOL, externalBlock } from "./externalguard.ts";
import type { Host } from "./host.ts";
import { PROTOCOL_NAME as MAIN_PROTOCOL, primaryCheckoutMove, primaryCheckoutWrite, reasonText as mainReasonText } from "./mainguard.ts";
import { blocked, type Verdict } from "./refusal-report.ts";
import { PROTOCOL_NAME as SECRET_PROTOCOL, reasonText as secretReasonText, secretBypass } from "./secretguard.ts";

/**
 * commands.md §10.18, contract §5.2: a guard's refusal also records one `protocol-breach` Event in the Ledger,
 * with the protocol it enforces, the guard, the refused tool and the refused command line (none for a question).
 */
export function reportProtocolBreach(
  host: Host,
  protocol: string,
  source: string,
  tool: string,
  command: string | undefined,
  reason: string,
): void {
  const id = host.run?.id ?? host.env.CPD_SESSION;
  const args = [
    "session",
    "report",
    "--kind",
    "protocol-breach",
    "--protocol",
    protocol,
    "--source",
    source,
    "--tool",
    tool,
    "--message",
    reason,
    ...(command === undefined ? [] : ["--command", command]),
    ...(id === undefined ? [] : ["--session", id]),
  ];
  host.call(args).catch((e) => host.deliver("protocol breach", e)); // L4: a failed call is logged, never retried
}

function breachBlocked(host: Host, protocol: string, source: string, tool: string, reason: string, input: unknown, command?: string): Verdict {
  reportProtocolBreach(host, protocol, source, tool, command, reason);
  return blocked(host, tool, reason, input);
}

/**
 * Evaluates the guards whose refusals report a `protocol-breach` Event (externalguard, mainguard, secretguard, askguard).
 * Returns the blocked Verdict if any guard refuses, or undefined if none refused.
 */
export function guardProtocolBreach(
  host: Host,
  event: { toolName: string; input: unknown; parentToolCallId?: string; toolCallId: string },
  ctx: { cwd: string; sessionManager: { getBranch(): readonly unknown[] } },
): Verdict {
  const role = host.run?.info.role;
  const external = host.run && externalBlock(event.toolName, event.input as Record<string, unknown>, { grantable: role !== "worker" });
  if (external) {
    const command = typeof (event.input as { command?: unknown }).command === "string" ? (event.input as { command: string }).command : undefined;
    return breachBlocked(host, EXTERNAL_PROTOCOL, "externalguard", event.toolName, external, event.input, command);
  }
  const home = homedir();
  if (host.run && (event.toolName === "edit" || event.toolName === "write")) {
    const written = primaryCheckoutWrite(event.toolName, event.input, ctx.cwd, home);
    if (written !== undefined) return breachBlocked(host, MAIN_PROTOCOL, "mainguard", event.toolName, mainReasonText(written), event.input);
  }
  const bashCommand = (event.input as { command?: unknown }).command;
  if (host.run && event.toolName === "bash" && typeof bashCommand === "string") {
    const moved = primaryCheckoutMove(bashCommand, ctx.cwd, home);
    if (moved !== undefined) return breachBlocked(host, MAIN_PROTOCOL, "mainguard", "bash", mainReasonText(moved), event.input, bashCommand);
    const secret = secretBypass(bashCommand);
    if (secret !== undefined) return breachBlocked(host, SECRET_PROTOCOL, "secretguard", "bash", secretReasonText(secret), event.input, bashCommand);
  }
  if (host.run && event.toolName === "ask_user_question") {
    const long = tooLong(event.input);
    if (long !== undefined) return breachBlocked(host, ASK_PROTOCOL, "askguard", "ask_user_question", long, event.input);
    const unseenReason = role === "lead" || role === "manager" ? unseen(event.input, event.parentToolCallId ?? event.toolCallId, ctx.sessionManager.getBranch()) : undefined;
    if (unseenReason !== undefined) return breachBlocked(host, ASK_PROTOCOL, "askguard", "ask_user_question", unseenReason, event.input);
    host.run.facts.hold(event.input); // §15.4: not refused, so a Verdict option names the dialog it opens
  }
  return undefined;
}
