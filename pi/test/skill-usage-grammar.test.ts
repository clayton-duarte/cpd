import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { grammar, usageError } from "./usage.ts";

const root = new URL("../../", import.meta.url);

function read(path: string): string {
  return readFileSync(new URL(path, root), "utf8");
}

function filesOf(dir: string): string[] {
  if (!existsSync(new URL(dir, root))) return [];
  return readdirSync(new URL(dir, root)).sort().map((f) => dir + f);
}

/** Every `cpd ...` command in code spans and fenced lines of the text. */
function commands(text: string): string[] {
  const found: string[] = [];
  let fenced = false;
  for (const line of text.split("\n")) {
    if (/^\s*```/.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) {
      const t = line.trim();
      if (t.startsWith("cpd ")) found.push(t);
      continue;
    }
    for (const m of line.matchAll(/`([^`]+)`/g)) {
      if (m[1].startsWith("cpd ")) found.push(m[1]);
    }
  }
  return found.filter((c) => !c.startsWith("cpd adapter:"));
}

function commandKey(cmd: string): string | undefined {
  const words: string[] = [];
  for (const tok of cmd.split(/\s+/).slice(1)) {
    if (/^[<\["$(-]/.test(tok)) break;
    words.push(tok);
  }
  for (let n = words.length; n > 0; n--) {
    const key = words.slice(0, n).join(" ");
    if (key in grammar) return key;
  }
  return undefined;
}

test("the cpd skills name only flags that cpd-usage.json lists for the command", () => {
  const files = [
    "pi/skills/cpd-lead/SKILL.md",
    ...filesOf("pi/skills/cpd-lead/references/"),
    "pi/skills/cpd-manager/SKILL.md",
    ...filesOf("pi/skills/cpd-manager/references/"),
  ];
  let checked = 0;
  const failures: string[] = [];
  for (const file of files) {
    for (const cmd of commands(read(file))) {
      // `cpd reminder` is the adapter's hidden turn-end reminder, not a clap command.
      if (cmd.split(/\s+/)[1] === "reminder") continue;
      checked++;
      const key = commandKey(cmd);
      if (key === undefined) {
        failures.push(`${file}: no grammar entry for: ${cmd}`);
        continue;
      }
      for (const flag of cmd.match(/(?<=\s)--[a-z][a-z0-9-]*/g) ?? []) {
        if (!(flag in grammar[key].flags)) failures.push(`${file}: ${flag} is not a flag of "${key}": ${cmd}`);
      }
    }
  }
  assert.deepEqual(failures, []);
  assert.ok(checked > 100, `more than 100 commands checked, got ${checked}`);
});

test("every cpd workflow describe command in the docs and skills takes the key as its argument", () => {
  const counts: Record<string, number> = {};
  const failures: string[] = [];
  const files = [
    "docs/commands.md",
    "docs/contract.md",
    "pi/skills/cpd-lead/SKILL.md",
    ...filesOf("pi/skills/cpd-lead/references/"),
  ];
  for (const file of files) {
    for (const m of read(file).matchAll(/cpd workflow describe[^`\n]*/g)) {
      const text = m[0].trim();
      const argv = text
        .split(/\s+/)
        .slice(1)
        .map((t) => t.replace(/[\[\]]/g, "").replace(/<([^>]*)>/g, "$1"))
        .filter((t) => t !== "");
      if (argv.length === 2) continue; // bare reference to the command by name
      const bucket = file.startsWith("pi/skills/cpd-lead/references/") ? "references" : file;
      counts[bucket] = (counts[bucket] ?? 0) + 1;
      const err = usageError(argv);
      if (err !== undefined) failures.push(`${file}: ${text}: ${err}`);
    }
  }
  assert.deepEqual(failures, []);
  assert.ok((counts["docs/commands.md"] ?? 0) >= 2, "at least 2 in docs/commands.md");
  assert.ok((counts["docs/contract.md"] ?? 0) >= 2, "at least 2 in docs/contract.md");
  assert.ok((counts["pi/skills/cpd-lead/SKILL.md"] ?? 0) >= 6, "at least 6 in cpd-lead SKILL.md");
  assert.ok((counts["references"] ?? 0) >= 1, "at least 1 in cpd-lead references");
});
