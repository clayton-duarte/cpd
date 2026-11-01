import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { namespacesFor } from "../../durable-host/codemode.ts";
import { createReload, type ReloadGuard, type ReloadPort } from "../../durable-host/reload.ts";
import { createRetier, type RetierPort } from "../../durable-host/retier.ts";
import { skillsCheck, skillsSection, type SkillsPort } from "../../durable-host/skills.ts";
import { reserveTokensOf } from "../../lib/compaction-ratio.ts";
import { noticeContent, skillChanges } from "../../lib/skills.ts";
import { withHost } from "./rig.ts";

// Contract §6.18 operations, pure over fakes: no socket, no timer, no pi-durable run, no model.
// Every test drives a stub (reload.ts, retier.ts, codemode.ts, skills.ts, compaction-ratio.ts), so
// each fails on 'not implemented' (or on the missing wire) until the implementation Job lands.

/** A temporary role skill directory: `<root>/cpd-lead/SKILL.md` with a name and a description. */
function skillsDir(t: TestContext): string {
  const root = mkdtempSync(join(tmpdir(), "cpd-p19-skills-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const dir = join(root, "cpd-lead");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "SKILL.md"), "---\nname: cpd-lead\ndescription: The Lead role\n---\n\nBody\n");
  return dir;
}

const refusal = (e: unknown): { code?: string; message?: string; details?: { reason?: string } } => e as never;

test("reload validates the package and installs the new extension answering reloaded true", async () => {
  const rebuilt = { name: "cpd-host-rebuilt" };
  const validated: unknown[] = [];
  const installed: unknown[] = [];
  const port: ReloadPort = {
    rebuild: () => rebuilt,
    guard: () => ({ busy: false, pending: false, state: "idle" }),
    validate: (extension) => {
      validated.push(extension);
      return { ok: true };
    },
    install: async (extension) => {
      installed.push(extension);
    },
  };
  const answer = await createReload(port).reload();
  assert.deepEqual(answer, { reloaded: true });
  assert.deepEqual(validated, [rebuilt], "the rebuilt extension was not validated");
  assert.deepEqual(installed, [rebuilt], "the rebuilt extension was not installed");
});

test("reload refuses with the named reason while busy pending or not idle", async () => {
  const port = (over: Partial<ReloadPort>): ReloadPort => ({
    rebuild: () => ({ name: "rebuilt" }),
    guard: () => ({ busy: false, pending: false, state: "idle" }),
    validate: () => ({ ok: true }),
    install: async () => {},
    ...over,
  });
  const cases: [ReloadGuard, string][] = [
    [{ busy: true, pending: false, state: "idle" }, "busy"],
    [{ busy: false, pending: true, state: "idle" }, "pending"],
    [{ busy: false, pending: false, state: "working" }, "state"],
  ];
  for (const [guard, reason] of cases) {
    await assert.rejects(
      createReload(port({ guard: () => guard })).reload(),
      (e: unknown) => {
        const err = refusal(e);
        return err.code === "refused" && err.details?.reason === reason && (err.message ?? "").includes(reason);
      },
      `the ${reason} refusal`,
    );
  }
  await assert.rejects(
    createReload(port({ validate: () => ({ ok: false, reason: "entry files missing" }) })).reload(),
    (e: unknown) => {
      const err = refusal(e);
      return err.code === "refused" && err.details?.reason === "entry files missing" && (err.message ?? "").includes("entry files missing");
    },
    "the validation refusal",
  );
});

test("the skills section names the role skill and its path", (t) => {
  const dir = skillsDir(t);
  const section = skillsSection("cpd-lead", dir);
  assert.ok(section !== undefined, "skillsSection rendered no section");
  assert.match(section, /cpd-lead/);
  assert.ok(section.includes(join(dir, "SKILL.md")), `the section must name the SKILL.md path, got:\n${section}`);
});

test("a changed skills digest sends the skill changed notice", async (t) => {
  const dir = skillsDir(t);
  const old = "0".repeat(64);
  const order: string[] = [];
  const writes: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }[] = [];
  const notices: { type: string; level: string; message: string }[] = [];
  const port: SkillsPort = {
    entries: async () => [{ kind: "cpd-skills", data: { skill: "cpd-lead", files: { "SKILL.md": old } } }],
    write: async (entry) => {
      order.push("write");
      writes.push(entry);
      return {};
    },
    broadcast: (msg) => {
      order.push("broadcast");
      notices.push(msg);
    },
    deliver: (error) => {
      throw new Error(`unexpected deliver: ${error.code}`);
    },
    log: () => {},
  };
  await skillsCheck(port, "cpd-lead", dir);
  assert.equal(writes.length, 1, "the new baseline entry was not written");
  const entry = writes[0]!;
  assert.equal(entry.kind, "cpd-skills");
  assert.equal(entry.data.skill, "cpd-lead");
  const files = entry.data.files as Record<string, string>;
  assert.equal(typeof files["SKILL.md"], "string");
  assert.notEqual(files["SKILL.md"], old, "the baseline was not updated to the new digest");
  assert.deepEqual(notices, [{ type: "notice", level: "info", message: "skills changed" }]);
  assert.deepEqual(order, ["write", "broadcast"], "the new baseline is written before the notice");
  const expected = noticeContent({ skill: "cpd-lead", dir, changes: skillChanges({ "SKILL.md": old }, files), files });
  assert.ok(JSON.stringify(entry.model).includes(expected), `the model entry must carry the exact notice text, got ${JSON.stringify(entry.model)}`);
});

test("a session retiered event applies the agent change at idle in one commit and reports switched", async () => {
  const configured: unknown[] = [];
  const reports: { kind: string; detail?: string }[] = [];
  const port: RetierPort = {
    idle: () => true,
    configure: async (agentChange) => {
      configured.push(agentChange);
    },
    report: async (kind, detail) => {
      reports.push({ kind, detail });
    },
  };
  const change = { model: { provider: "anthropic", modelId: "opus" }, thinkingLevel: "high" };
  await createRetier(port).apply(change);
  assert.deepEqual(configured, [change], "configure was not called once with the agent change");
  assert.deepEqual(reports, [{ kind: "switched", detail: undefined }]);
});

test("a retier while a run is pending is refused and reports switch-failed", async () => {
  const configured: unknown[] = [];
  const reports: { kind: string; detail?: string }[] = [];
  const port: RetierPort = {
    idle: () => false,
    configure: async (agentChange) => {
      configured.push(agentChange);
    },
    report: async (kind, detail) => {
      reports.push({ kind, detail });
    },
  };
  await createRetier(port).apply({ model: { provider: "anthropic", modelId: "opus" }, thinkingLevel: "high" });
  assert.deepEqual(configured, [], "nothing is configured while a run is pending");
  assert.equal(reports.length, 1);
  assert.equal(reports[0]!.kind, "switch-failed");
  assert.ok(typeof reports[0]!.detail === "string" && reports[0]!.detail!.length > 0, "switch-failed carries a detail");
});

test("the compaction ratio yields the reserve of the window by the lib rules", () => {
  const m = { provider: "anthropic", modelId: "sonnet", contextWindow: 200000 };
  assert.equal(reserveTokensOf({}, m), 20000, "the built-in ratio 0.1");
  assert.equal(reserveTokensOf({ reserveTokensRatio: 0.25 }, m), 50000, "the global ratio");
  assert.equal(reserveTokensOf({ modelOverrides: { "anthropic/sonnet": { reserveTokensRatio: 0.5 } } }, m), 100000, "the model ratio");
  assert.equal(reserveTokensOf({ modelOverrides: { "anthropic/sonnet": { reserveTokens: 12345 } } }, m), 12345, "the model absolute");
  assert.equal(reserveTokensOf({ reserveTokens: 7777 }, m), 7777, "the global absolute when no ratio applies");
  assert.equal(reserveTokensOf({ reserveTokens: 7777 }, { provider: "x", modelId: "y" }), 7777, "no positive window keeps the non-ratio value");
  assert.throws(() => reserveTokensOf({ reserveTokensRatio: 2 }, m), /compaction\.reserveTokensRatio/);
  assert.throws(() => reserveTokensOf({ modelOverrides: { "anthropic/sonnet": { reserveTokensRatio: 1 } } }, m), /compaction\.modelOverrides\["anthropic\/sonnet"\]\.reserveTokensRatio/);
});

test("the code mode extension registers the codemode tool and the mcp server namespaces", async () => {
  assert.deepEqual(
    namespacesFor([
      { server: "github", tools: ["mcp__github__search", "mcp__github__create_issue"] },
      { server: "linear", tools: ["mcp__linear__issue"] },
    ]),
    { "mcp__github__": ["mcp__github__search", "mcp__github__create_issue"], "mcp__linear__": ["mcp__linear__issue"] },
  );
  await withHost({}, async (r) => {
    await r.ready();
    const extension = r.extensions[0];
    assert.ok(extension !== undefined, "the host never opened a conversation");
    assert.ok(extension.tools?.some((tool) => tool.name === "codemode"), "the codemode tool is not registered in the host extension");
  });
});
