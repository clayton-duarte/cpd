/** commands.md §15.<n>: the tool `cpd_host_approve` and the raw-`cpd host approve` guard. Stub: the implementation is a later Job's. */
import type { Host } from "./host.ts";

/** commands.md §15.<n>: registers the tool `cpd_host_approve` (once per host). */
export function registerHostApproveTool(_host: Host): void {}

/** commands.md §15.<n>: the refused command as the reason shows it, or undefined when it may run. */
export function hostApproveBypass(_command: string): string | undefined {
  return undefined;
}
