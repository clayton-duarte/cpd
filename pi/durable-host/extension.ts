import { defineExtension, hook, section, ToolTask, type Extension, type HookResult, type JsonObject } from "@earendil-works/pi-durable";
import { POLICY_SECTION } from "../lib/policy.ts";
import { TIME_ZONE_SECTION } from "../lib/time.ts";
import { hostVerdict, type Where } from "./guards.ts";

export function hostExtension(prompt: { contextFiles: string | undefined; policy: string | undefined; zone: string }, where: Where, role: "lead" | "manager"): Extension {
  const texts: [string, string | undefined][] = [
    ["cpd-context-files", prompt.contextFiles],
    [POLICY_SECTION, prompt.policy],
    [TIME_ZONE_SECTION, prompt.zone],
  ];
  return defineExtension({
    name: "cpd-host",
    sections: texts.flatMap(([key, text]) => (text === undefined ? [] : [section(key, () => text)])),
    // The pure verdict's `arguments` is the contract's `Record<string, unknown>`; pi-durable's hook wants a `JsonObject`.
    hooks: [hook(ToolTask, { beforeTool: (call) => hostVerdict(call, where, role) as unknown as HookResult<{ arguments?: JsonObject; block?: string }> })],
  });
}
