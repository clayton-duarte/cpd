import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// docs/architecture/pi-native-core.md §4.6 & §7 (Plan 3: skills-to-tools).
// Pins:
// - Skills instruct agents to call typed pi tools instead of bash CLI subcommands
// - Skills instruct agents to use message tool (delegate and escalate) instead of board post or session send
// - Command lines across the skills are replaced by typed tool calls (e.g. cpd_plan, cpd_job, cpd_context, cpd_message)
// - Legacy board post instructions are eliminated
// - The skills no longer instruct falling back to bash CLI subcommands

const leadSkillPath = new URL("../skills/cpd-lead/SKILL.md", import.meta.url);
const managerSkillPath = new URL("../skills/cpd-manager/SKILL.md", import.meta.url);

test("skills use typed tools instead of cli", () => {
  const lead = readFileSync(leadSkillPath, "utf8");
  const manager = readFileSync(managerSkillPath, "utf8");

  // 1. Verify typed tool instructions exist
  for (const text of [lead, manager]) {
    assert.match(
      text,
      /## Typed tools/,
      "Skill must contain a ## Typed tools section",
    );
    assert.match(
      text,
      /\bdelegate\b/,
      "Skill must teach delegate",
    );
  }

  // 2. Verify legacy board post is completely removed
  assert.doesNotMatch(
    lead,
    /cpd board post/,
    "cpd-lead must not instruct using legacy cpd board post",
  );
  assert.doesNotMatch(
    manager,
    /cpd board post/,
    "cpd-manager must not instruct using legacy cpd board post",
  );

  // 3. Verify tool-first migration for state commands
  // Once skill migration is fully implemented, skills must not teach running
  // bash CLI subcommands for operations that have typed tools (cpd context decide, cpd plan show).
  // Currently, the skill still instructs:
  // "when no typed tool of that domain is in your tool list, run that command line in bash instead"
  // and retains CLI command lines instead of typed tool calls.
  // When migrated, typed tools are required and active.
  const isMigratedToTypedToolsOnly =
    !lead.includes("run that command line in bash instead") &&
    !manager.includes("run that command line in bash instead");

  assert.ok(
    isMigratedToTypedToolsOnly,
    "Skills must require typed pi tools and not fall back to CLI subcommands in bash",
  );
});
