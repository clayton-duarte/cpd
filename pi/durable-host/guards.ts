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
};

/** Contract §6.16: a block, a rewritten call, or nothing. The rewrite arm carries the new `arguments`. */
export type Verdict = { block: string } | { arguments: Record<string, unknown> } | undefined;

/**
 * The Lead or Manager verdict (contract §6.16): the four shared guards first and unchanged, then noblock (G24), the ask guards
 * (G25, G26), and no test-guard arm (G27). `role` picks the noblock reason tail; `where.visible` is the chat the G4 guard reads.
 */
export function hostVerdict(
  call: { id: string; name: string; arguments: Record<string, unknown> },
  where: Where,
  role: "lead" | "manager",
): Verdict {
  if (call.name === "edit" || call.name === "write") {
    const written = primaryCheckoutWrite(call.name, call.arguments, where.cwd, where.home);
    return written === undefined ? undefined : { block: mainReason(written) };
  }
  const command = call.arguments.command;
  if (call.name !== "bash" || typeof command !== "string") {
    // The ask guards (G25, G26) are for an `ask_user_question` call only; every other tool passes.
    if (call.name === "ask_user_question") throw new Error("not implemented");
    return undefined;
  }
  const moved = primaryCheckoutMove(command, where.cwd, where.home);
  if (moved !== undefined) return { block: mainReason(moved) };
  const secret = secretBypass(command);
  if (secret !== undefined) return { block: secretReason(secret) };
  const host = hostApproveBypass(command);
  if (host !== undefined) return { block: hostReason(host) };
  // noblock (G24): the new branch is not implemented by this Worker.
  throw new Error("not implemented");
}
