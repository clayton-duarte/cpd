import type { WireError } from "../protocol/domain.ts";

const HOST_CODES: ReadonlySet<string> = new Set([
  "store_too_new", "spawn_failed", "adapter_missing", "gh_failed", "slack_auth",
  "slack_not_configured", "jira_auth", "jira_not_configured", "classifier_unreachable",
]);
const HOST_MESSAGE = /store_too_new|Upgrade cpd|unreachable|needs a sign-in/i;

/** Who the error toast reports to: host for environment faults, cpd for code faults (CONTRACT 11.7). */
export function routeErrorTarget(error: WireError): "cpd" | "host" {
  if ((error.code as string) === "cpd_unavailable") return "host";
  const cpdCode = error.details?.["cpdCode"];
  if (typeof cpdCode === "string" && HOST_CODES.has(cpdCode)) return "host";
  return HOST_MESSAGE.test(error.message) ? "host" : "cpd";
}
