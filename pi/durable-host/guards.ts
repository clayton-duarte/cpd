import type { Host } from "../lib/host.ts";
import { toolCallGuard } from "../lib/loop-guard.ts";
import { blockingReason, reasonText as noblockReason, BOUND_SECONDS } from "../lib/noblock.ts";
import { tooLong, unseen } from "../lib/askguard.ts";
import { hostApproveBypass, reasonText as hostReason } from "../lib/hostapprove.ts";
import { primaryCheckoutMove, primaryCheckoutWrite, reasonText as mainReason } from "../lib/mainguard.ts";
import { reasonText as secretReason, secretBypass } from "../lib/secretguard.ts";

export type Where = {
  cwd: string;
  home: string;
  /** The session's visible chat, in the shape `unseen` reads (contract §6.16, askguard.ts). */
  visible: readonly unknown[];
  /** The loop guard's host (the lib's `toolCallGuard` reads `host.run` and `host.options.now`); absent: no per-turn bound. */
  loop?: Host;
};

/** Contract §6.16: a block, a rewritten call, or nothing. The rewrite arm carries the new `arguments`. */
export type Verdict = { block: string } | { arguments: Record<string, unknown> } | undefined;

/**
 * The Lead or Manager verdict (contract §6.16): the four shared guards first and unchanged, then noblock (G24), the ask guards
 * (G25, G26), and no test-guard arm (G27). `role` picks the noblock reason tail; `where.visible` is the chat the G4 guard reads.
 */
export function hostVerdict(
  call: { id: string; name: string; arguments: Record<string, unknown>; parent?: { readonly taskId: unknown; readonly callId: string } },
  where: Where,
  role: "lead" | "manager",
): Verdict {
  // The per-turn bounds (contract §6.16) first, as the adapter does; a nested call (`parent`) is neither counted nor refused.
  if (where.loop !== undefined) {
    const refused = toolCallGuard(where.loop, { toolName: call.name, input: call.arguments, parentToolCallId: call.parent === undefined ? undefined : call.parent.callId });
    if (refused !== undefined) return { block: refused };
  }
  if (call.name === "edit" || call.name === "write") {
    const written = primaryCheckoutWrite(call.name, call.arguments, where.cwd, where.home);
    return written === undefined ? undefined : { block: mainReason(written) };
  }
  if (call.name === "bash") {
    const command = call.arguments.command;
    if (typeof command !== "string") return undefined;
    const moved = primaryCheckoutMove(command, where.cwd, where.home);
    if (moved !== undefined) return { block: mainReason(moved) };
    const secret = secretBypass(command);
    if (secret !== undefined) return { block: secretReason(secret) };
    const host = hostApproveBypass(command);
    if (host !== undefined) return { block: hostReason(host) };
    const timeout = call.arguments.timeout;
    const matched = blockingReason(command, typeof timeout === "number" ? timeout : undefined);
    if (matched !== undefined) return { block: noblockReason(matched, role) };
    // noblock (G24): no match and no explicit timeout: bound the foreground call. An explicit one is kept unchanged.
    return timeout === undefined ? { arguments: { ...call.arguments, timeout: BOUND_SECONDS } } : undefined;
  }
  if (call.name === "ask_user_question") {
    // The ask guards (G25, G26) are for an `ask_user_question` call only; every host Session is a Lead or Manager.
    const long = tooLong(call.arguments);
    if (long !== undefined) return { block: long };
    const missing = unseen(call.arguments, call.id, where.visible);
    return missing === undefined ? undefined : { block: missing };
  }
  return undefined;
}
