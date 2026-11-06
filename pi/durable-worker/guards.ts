import { exploreVerdict, isInsideRepo, type ExploreKind } from "../lib/explore.ts";
import { externalBlock } from "../lib/externalguard.ts";
import { hostApproveBypass, reasonText as hostReason } from "../lib/hostapprove.ts";
import { primaryCheckoutMove, primaryCheckoutWrite, reasonText as mainReason } from "../lib/mainguard.ts";
import { reasonText as secretReason, secretBypass } from "../lib/secretguard.ts";
import { reasonText as testReason, unfilteredTestRun } from "../lib/testguard.ts";

export type Where = {
  cwd: string;
  home: string; // the User's home directory, os.homedir(); read by the primary-checkout guards
  cpdHome: string; // $CPD_HOME, Config.home; read by the explore profile only
  kind?: ExploreKind;
  plan?: string;
};

export type Verdict = { block: string } | undefined;

export function workerVerdict(call: { name: string; arguments: Record<string, unknown> }, where: Where): Verdict {
  const external = externalBlock(call.name, call.arguments, { grantable: false });
  if (external !== undefined) return { block: external };
  if (where.kind !== undefined) {
    const refused = exploreVerdict(where.kind, call, { cwd: where.cwd, home: where.cpdHome, plan: where.plan ?? "" }, isInsideRepo);
    if (refused !== undefined) return refused;
  }
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
  if (host !== undefined) return { block: hostReason(host) };
  const test = unfilteredTestRun(command);
  return test === undefined ? undefined : { block: testReason(test) };
}
