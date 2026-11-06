import { describe, expect, it } from "vitest";
import type { WireError } from "../protocol/domain.ts";
import { routeErrorTarget } from "./errorReport.ts";

const cpdCodes = ["store_too_new", "spawn_failed", "adapter_missing", "gh_failed", "slack_auth", "slack_not_configured", "jira_auth", "jira_not_configured", "classifier_unreachable"];
const failed = (cpdCode: string, message = "m"): WireError => ({ code: "cpd_failed", message, details: { cpdCode } });

describe("routeErrorTarget", () => {
  it("routeErrorTarget sends host-class errors to host", () => {
    expect(routeErrorTarget({ code: "cpd_unavailable" as WireError["code"], message: "m" })).toBe("host");
    for (const c of cpdCodes) expect(routeErrorTarget(failed(c)), c).toBe("host");
    for (const m of ["store_too_new here", "Upgrade cpd now", "host unreachable", "needs a sign-in"]) {
      expect(routeErrorTarget(failed("other", m)), m).toBe("host");
    }
  });
  it("routeErrorTarget sends code and protocol errors to cpd", () => {
    expect(routeErrorTarget({ code: "protocol_error", message: "m" })).toBe("cpd");
    expect(routeErrorTarget({ code: "cpd_malformed_json", message: "m" })).toBe("cpd");
    expect(routeErrorTarget({ code: "invalid_message", message: "m" })).toBe("cpd");
    expect(routeErrorTarget({ code: "unknown_action", message: "m" })).toBe("cpd");
    expect(routeErrorTarget(failed("item_not_found", "no such item"))).toBe("cpd");
  });
});
