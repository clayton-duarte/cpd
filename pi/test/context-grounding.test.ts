import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const skill = (name: string) => readFileSync(new URL(`../skills/${name}/SKILL.md`, import.meta.url), "utf8");

test("Manager grounds product work in the User's material and existing decisions", () => {
  const text = skill("cpd-manager");
  assert.match(text, /Read the User's material and relevant existing decisions/);
  assert.match(text, /Record settled decisions and durable context/);
  assert.match(text, /keep unresolved questions open/i);
});

test("Lead grounds delivery in the brief, decisions and relevant material", () => {
  const text = skill("cpd-lead");
  assert.match(text, /Read the brief, relevant decisions, and the material needed/);
  assert.match(text, /Investigate factual uncertainty/);
  assert.match(text, /ask the Manager or User only when product intent, priority, or authority is unresolved/);
});
