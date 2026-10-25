import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

const root = join(import.meta.dirname, "..", "..");
const TAGS = ["m-units", "m-session", "m-engine", "m-tui-r", "m-tui-t", "m-flow", "m-pr-a", "m-pr-b", "m-tooling", "contract"];

function lines(): string[] {
  const file = join(root, "scripts", "tripwire", "allow.txt");
  assert.ok(existsSync(file), "scripts/tripwire/allow.txt does not exist");
  const all = readFileSync(file, "utf8").split("\n").filter((l) => l.trim() !== "" && !l.trimStart().startsWith("#"));
  assert.ok(all.length >= 1, "scripts/tripwire/allow.txt has no entry");
  return all;
}

test("every allow.txt line is well formed and tagged with a live PR key", () => {
  const all = lines();
  const bad = all.filter((l) => {
    const m = /^(\S+) (\S+)  # ([a-z-]+): \S.*$/.exec(l);
    return !m || !TAGS.includes(m[3]);
  });
  assert.deepEqual(bad, [], "malformed or untagged lines");
  assert.equal(new Set(all).size, all.length, "duplicate lines");
});

test("every allow.txt prefix names a test module that exists", () => {
  const missing: string[] = [];
  for (const l of lines()) {
    const [id, prefix] = l.split(/\s+/);
    const first = prefix.split("::")[0];
    const crate = /^(cpd-core|cpd-engine|cpd-tui)$/.exec(id);
    const area = /^cpd::(\w+)$/.exec(id);
    let ok = false;
    if (area) ok = existsSync(join(root, "cli", "tests", area[1], `${first}.rs`)) || existsSync(join(root, "cli", "tests", area[1], first));
    else if (crate) {
      const src = { "cpd-core": "core", "cpd-engine": "engine", "cpd-tui": "tui" }[id]!;
      ok = existsSync(join(root, src, "src", `${first}.rs`)) || existsSync(join(root, src, "src", first));
    }
    if (!ok) missing.push(l);
  }
  assert.deepEqual(missing, [], "prefixes naming no test module (or an unknown binary id)");
});
