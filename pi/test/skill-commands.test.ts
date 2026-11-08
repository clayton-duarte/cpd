import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { BOUND_SECONDS, guard, guardExamples } from "../lib/command-guard.ts";
import type { SkillFile } from "../lib/command-guard.ts";

// contract.md §8.3 "Delivery to agents"; docs/implementing.md "The command-example guard"
// (Plan `agent-refusals`, Workflow `enforce`): every command example the skills and their
// Spec references teach must be one the Session that copies it could actually run. The guard
// is pure: it reads text and runs nothing. Scoping (assumption A3): the 30 s shell bound
// applies to Lead, Manager and explore context; a Worker or PR acceptance example is bounded
// by `attempt_minutes`, not 30 s.

const skillsDir = new URL("../skills/", import.meta.url);

/** The skills whose text the guard scans: `writing-for-agents/` is upstream's unchanged text and is never scanned. */
const skills = ["cpd-lead", "cpd-manager"] as const;

/** A skill's SKILL.md plus every file of its references/ directory (sorted), the same set `skill.test.ts` scans. */
function skillFiles(skill: string): SkillFile[] {
  const dir = new URL(`${skill}/`, skillsDir);
  const refs = new URL("references/", dir);
  const refNames = existsSync(refs) ? readdirSync(refs).sort() : [];
  return [
    { path: `${skill}/SKILL.md`, text: readFileSync(new URL("SKILL.md", dir), "utf8") },
    ...refNames.map((name) => ({ path: `${skill}/references/${name}`, text: readFileSync(new URL(name, refs), "utf8") })),
  ];
}

test("the command-example guard flags a Lead example over 30 s", () => {
  assert.equal(BOUND_SECONDS, 30, "the bound is noblock's 30 s");
  const lead: SkillFile = {
    path: "cpd-lead/SKILL.md",
    text: ["Report done when the build is green:", "", "```sh", "timeout 60 npm run build", "```", ""].join("\n"),
  };
  const violations = guardExamples([lead]);
  assert.equal(violations.length, 1, `the guard must flag the one over-30 s Lead example, it reported ${violations.length}`);
  const v = violations[0]!;
  assert.equal(v.code, "timeout", "the runtime refuses it as a wait over the bound (noblock G24, commands.md §15.11)");
  assert.match(v.reason, /timeout 60/, `the reason names the refused example: ${v.reason}`);
});

test("every command example in the skills passes the guard", () => {
  const files = skills.flatMap(skillFiles);
  const result = guard(files);
  assert.ok(
    result.examples.length > 0,
    "the guard must collect the skills' command examples, not report an empty scan",
  );
  const flagged = result.violations.map((v) => `${v.file}:${v.line}: ${v.code}: ${v.text}`);
  assert.deepEqual(result.violations, [], `the skills teach only examples a Session could run:\n${flagged.join("\n")}`);
});
