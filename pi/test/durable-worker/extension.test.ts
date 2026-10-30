import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import type { Extension, PromptSection, ToolHooks } from "@earendil-works/pi-durable";
import { POLICY_SECTION } from "../../lib/policy.ts";
import { TIME_ZONE_SECTION } from "../../lib/time.ts";
import { reasonText } from "../../lib/testguard.ts";
import { cpdExtension } from "../../durable-worker/extension.ts";

const WHERE = { cwd: "/nowhere", home: "/nohome", cpdHome: "/nocpd" };

/** What a section renders; the Harness's input and context are not used by cpd's sections, so none is opened. */
const render = async (s: PromptSection) => s.render(undefined as never, undefined as never);
const beforeTool = (e: Extension): ToolHooks["beforeTool"] => {
  const reg = e.hooks?.[0];
  assert.ok(reg, "one hook");
  return (reg.handlers as ToolHooks).beforeTool;
};

test("cpdExtension has the context-files, policy and time-zone sections in that order", async () => {
  const e = cpdExtension({ contextFiles: "## AGENTS.md\nrules", policy: "## pr (x)\ntext", zone: "Time zone: X" }, WHERE);
  assert.equal(e.name, "cpd");
  const sections = e.sections ?? [];
  assert.deepEqual(sections.map((s) => s.key), ["cpd-context-files", POLICY_SECTION, TIME_ZONE_SECTION]);
  assert.deepEqual(await Promise.all(sections.map(render)), ["## AGENTS.md\nrules", "## pr (x)\ntext", "Time zone: X"]);
});

test("cpdExtension omits a section whose text is undefined", async () => {
  const e = cpdExtension({ contextFiles: undefined, policy: "p", zone: "z" }, WHERE);
  assert.deepEqual((e.sections ?? []).map((s) => s.key), [POLICY_SECTION, TIME_ZONE_SECTION]);
  const none = cpdExtension({ contextFiles: undefined, policy: undefined, zone: "z" }, WHERE);
  assert.deepEqual((none.sections ?? []).map((s) => s.key), [TIME_ZONE_SECTION]);
});

test("cpdExtension beforeTool blocks with the guard's reason and passes an allowed call", async (t) => {
  const tmp = mkdtempSync(join(tmpdir(), "durable-ext-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  mkdirSync(join(tmp, "plain"));
  const hook = beforeTool(cpdExtension({ contextFiles: undefined, policy: undefined, zone: "z" }, { cwd: join(tmp, "plain"), home: tmp, cpdHome: join(tmp, "cpd") }));
  const call = (command: string) => ({ type: "toolCall" as const, id: "c1", name: "bash", arguments: { command } });
  const api = undefined as never;
  assert.deepEqual(await hook(call("cargo test --workspace"), api, api), { block: reasonText("cargo test --workspace") });
  assert.equal(await hook(call("ls"), api, api), undefined);
});
