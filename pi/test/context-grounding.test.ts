import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const skillText = (skill: string): string => readFileSync(new URL(`../skills/${skill}/SKILL.md`, import.meta.url), "utf8");

/** The code spans of a text. */
const spans = (text: string): string[] => [...text.matchAll(/`([^`\n]+)`/g)].map((m) => m[1]!);

const targeted: [string, RegExp][] = [
  ["cpd context search", /^cpd context search\b/],
  ["cpd context show --topic", /^cpd context show --topic\b/],
  ["cpd context show --record", /^cpd context show --record\b/],
  ["cpd context show --since", /^cpd context show --since\b/],
];
const all = /^cpd context show --all\b/;
const topics = /^cpd context topics\b/;

function namesTargeted(skill: string): void {
  const named = spans(skillText(skill));
  for (const [label, re] of targeted) assert.ok(named.some((s) => re.test(s)), `${skill} names \`${label}\``);
  assert.ok(named.some((s) => all.test(s)), `${skill} names \`cpd context show --all\``);
  assert.ok(!named.some((s) => topics.test(s)), `${skill} must not name \`cpd context topics\``);
}

test("cpd-manager grounds on targeted context reads and never says a bare show renders every record", () => {
  assert.doesNotMatch(skillText("cpd-manager"), /renders every record/);
  namesTargeted("cpd-manager");
  assert.ok(!spans(skillText("cpd-lead")).some((s) => topics.test(s)), "cpd-lead must not name `cpd context topics`");
});

test("cpd-lead names the targeted context reads and the --all escape", () => {
  namesTargeted("cpd-lead");
});
