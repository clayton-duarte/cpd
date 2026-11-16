import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

const skills = ["cpd-lead", "cpd-manager"] as const;
function files(skill: string): { name: string; text: string }[] {
  const dir = new URL(`../skills/${skill}/`, import.meta.url);
  const refs = new URL("references/", dir);
  return ["SKILL.md", ...(existsSync(refs) ? readdirSync(refs).sort().map((f) => `references/${f}`) : [])]
    .map((name) => ({ name: `${skill}/${name}`, text: readFileSync(new URL(name, dir), "utf8") }));
}

test("role guidance coordinates dependencies and escalates product gaps", () => {
  const lead = files("cpd-lead")[0]!.text;
  const manager = files("cpd-manager")[0]!.text;
  assert.match(lead, /Coordinate actual dependencies/);
  assert.match(lead, /who can provide it, and a recovery or escalation route/);
  assert.match(lead, /Keep the Manager informed of product gaps/);
  assert.match(manager, /bring the concrete options and consequences to the User/);
  assert.match(manager, /return it to the Lead/);
});

test("skill examples do not teach invalid addresses or omit send justification", () => {
  for (const skill of skills) for (const f of files(skill)) {
    assert.doesNotMatch(f.text, /session rename|plan new[^\n]*--name|<manager-id>|<lead-session>/, f.name);
    for (const line of f.text.split("\n")) for (const m of line.matchAll(/cpd session send[^`\n]*/g)) {
      assert.match(m[0], /--unblocks/, `${f.name}: ${m[0]}`);
    }
  }
});
