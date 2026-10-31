import type { Extension } from "@earendil-works/pi-durable";
import type * as held from "./held.ts";
import type { Dialogs } from "./dialogs.ts";

// Contract §6.15 `tools.ts`: the host's registrations of `ask_user_question`, `cpd_secret_approve`, `cpd_host_approve`
// and `cpd_plan_review`, each a `defineTool` whose execute follows the pi reference (secret.ts, hostapprove.ts,
// review.ts, pi/ask-user-question). The types are real; the body is P1.6's, not this Job.

export type RegisterHostToolsWhere = {
  role: "lead" | "manager";
  plan: string | undefined;
  name: string;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  held: held.HeldApi;
  dialogs: Dialogs;
};

export function registerHostTools(where: RegisterHostToolsWhere): Extension {
  throw new Error("not implemented");
}