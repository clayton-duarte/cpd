import { hostApproveBypass, reasonText as hostReason } from "../lib/hostapprove.ts";
import { primaryCheckoutMove, primaryCheckoutWrite, reasonText as mainReason } from "../lib/mainguard.ts";
import { reasonText as secretReason, secretBypass } from "../lib/secretguard.ts";

export type Where = { cwd: string; home: string };

export type Verdict = { block: string } | undefined;

export function hostVerdict(call: { name: string; arguments: Record<string, unknown> }, where: Where): Verdict {
  if (call.name === "edit" || call.name === "write") {
    const written = primaryCheckoutWrite(call.name, call.arguments, where.cwd, where.home);
    return written === undefined ? undefined : { block: mainReason(written) };
  }
  const command = call.arguments.command;
  if (call.name !== "bash" || typeof command !== "string") return undefined;
  const moved = primaryCheckoutMove(command, where.cwd, where.home);
  if (moved !== undefined) return { block: mainReason(moved) };
  const secret = secretBypass(command);
  if (secret !== undefined) return { block: secretReason(secret) };
  const host = hostApproveBypass(command);
  return host === undefined ? undefined : { block: hostReason(host) };
}
