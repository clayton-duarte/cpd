import { defineExtension, hook, section, ToolTask, type Extension } from "@earendil-works/pi-durable";
import { POLICY_SECTION } from "../lib/policy.ts";
import { TIME_ZONE_SECTION } from "../lib/time.ts";
import { workerVerdict } from "./guards.ts";

export function cpdExtension(prompt: { contextFiles: string | undefined; policy: string | undefined; zone: string }, where: { cwd: string; home: string }): Extension {
  const texts: [string, string | undefined][] = [
    ["cpd-context-files", prompt.contextFiles],
    [POLICY_SECTION, prompt.policy],
    [TIME_ZONE_SECTION, prompt.zone],
  ];
  return defineExtension({
    name: "cpd",
    sections: texts.flatMap(([key, text]) => (text === undefined ? [] : [section(key, () => text)])),
    hooks: [hook(ToolTask, { beforeTool: (call) => workerVerdict(call, where) })],
  });
}
