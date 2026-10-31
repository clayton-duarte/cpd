import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import type { Extension, PromptSection, ToolHooks } from "@earendil-works/pi-durable";
import { POLICY_SECTION } from "../../lib/policy.ts";
import { TIME_ZONE_SECTION } from "../../lib/time.ts";
import { reasonText } from "../../lib/mainguard.ts";
import { hostExtension } from "../../durable-host/extension.ts";

const WHERE = { cwd: "/nowhere", home: "/nohome" };

const render = async (s: PromptSection) => s.render(undefined as never, undefined as never);
const beforeTool = (e: Extension): ToolHooks["beforeTool"] => {
  const reg = e.hooks?.[0];
  assert.ok(reg, "one hook");
  return (reg.handlers as ToolHooks).beforeTool;
};

test("hostExtension has the context-files, policy and time-zone sections", async () => {
  const e = hostExtension({ contextFiles: "## AGENTS.md\nrules", policy: "## pr (x)\ntext", zone: "Time zone: X" }, WHERE);
  const sections = e.sections ?? [];
  assert.deepEqual(sections.map((s) => s.key), ["cpd-context-files", POLICY_SECTION, TIME_ZONE_SECTION]);
  assert.deepEqual(await Promise.all(sections.map(render)), ["## AGENTS.md\nrules", "## pr (x)\ntext", "Time zone: X"]);
  const some = hostExtension({ contextFiles: undefined, policy: undefined, zone: "z" }, WHERE);
  assert.deepEqual((some.sections ?? []).map((s) => s.key), [TIME_ZONE_SECTION]);
});

test("hostExtension runs hostVerdict in its beforeTool hook", async (t) => {
  const tmp = mkdtempSync(join(tmpdir(), "durable-host-ext-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(tmp, "plain"));
  const hook = beforeTool(hostExtension({ contextFiles: undefined, policy: undefined, zone: "z" }, { cwd: join(tmp, "plain"), home: tmp }));
  const api = undefined as never;
  const abs = join(primary, "f");
  const edit = { type: "toolCall" as const, id: "c1", name: "edit", arguments: { path: abs } };
  assert.deepEqual(await hook(edit, api, api), { block: reasonText(`edit ${abs} in ${primary}`) });
  const bash = (command: string) => ({ type: "toolCall" as const, id: "c2", name: "bash", arguments: { command } });
  assert.equal(await hook(bash("cargo test --workspace"), api, api), undefined);
  assert.equal(await hook(bash("ls"), api, api), undefined);
});
