import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

test("Lead addresses failure causes before retry and reports genuine blockers", () => {
  const lead = readFileSync(new URL("../skills/cpd-lead/SKILL.md", import.meta.url), "utf8");
  assert.match(lead, /identify the responsible actor and a usable next step/);
  assert.match(lead, /retry only when the cause is addressed/);
  assert.match(lead, /report the blocker, its evidence, and the intervention needed/);
  assert.doesNotMatch(lead, /cancel[^.\n]*\badds? a fresh/i);
});
