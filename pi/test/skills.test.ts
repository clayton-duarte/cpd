import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const skill = (name: string) => readFileSync(new URL(`../skills/${name}/SKILL.md`, import.meta.url), "utf8");

test("role methodology respects rather than replaces runtime controls", () => {
  for (const name of ["cpd-lead", "cpd-manager"]) {
    const text = skill(name);
    assert.match(text, /do not replace or disable runtime controls|does not replace or disable runtime controls/, name);
    assert.match(text, /Follow (?:the )?controls (?:active|in force)/, name);
    assert.match(text, /(?:Never bypass a guard|Do not claim completion without evidence, bypass a control)/, name);
  }
});
