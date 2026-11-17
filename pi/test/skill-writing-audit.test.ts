import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

const root = new URL("../skills/", import.meta.url);
function files(skill: string): { name: string; text: string }[] {
  const dir = new URL(`${skill}/`, root);
  const refs = new URL("references/", dir);
  const names = existsSync(refs) ? readdirSync(refs).sort() : [];
  return ["SKILL.md", ...names.map((f) => `references/${f}`)]
    .map((name) => ({ name: `${skill}/${name}`, text: readFileSync(new URL(name, dir), "utf8") }));
}
const all = [...files("cpd-lead"), ...files("cpd-manager")];
const lead = all.find((f) => f.name === "cpd-lead/SKILL.md")!.text;
const manager = all.find((f) => f.name === "cpd-manager/SKILL.md")!.text;

test("skills do not leak record ids, dated history, or worker instructions", () => {
  for (const f of all) assert.doesNotMatch(f.text, /\b(hs|pro)-[dnqap][0-9]+|\b20\d\d-\d\d-\d\d\b|on the ledger of|renumber the later steps|trial t5/, f.name);
});

test("role descriptions identify their roles and remain concise", () => {
  for (const [name, text, max] of [["cpd-lead", lead, 330], ["cpd-manager", manager, 260]] as const) {
    const frontmatter = /^---\n([\s\S]*?)\n---\n/.exec(text);
    assert.ok(frontmatter, `${name}: frontmatter`);
    const description = /^description: (.+)$/m.exec(frontmatter[1]!);
    assert.ok(description, `${name}: description`);
    assert.ok(description[1]!.length <= max, `${name}: description too long`);
    assert.match(description[1]!, new RegExp(name.replace("cpd-", ""), "i"));
  }
});

test("role contracts require checkable evidence and preserve product decisions", () => {
  assert.match(lead, /checkable acceptance evidence/);
  assert.match(lead, /A passing test proves only what it tested/);
  assert.match(manager, /Record settled decisions and durable context/);
  assert.match(manager, /do not turn a guess into a requirement/);
});

test("skill text avoids malformed body placeholders and obsolete sleep advice", () => {
  for (const f of all) assert.doesNotMatch(f.text, /<body\.md(?!>)|sleep 10|opens only when you do/, f.name);
});
