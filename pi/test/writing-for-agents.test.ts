// Contract §8.3: the bundled writing-for-agents skill is a pinned, unchanged upstream copy.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const dir = fileURLToPath(new URL("../skills/writing-for-agents/", import.meta.url));

test("the copy holds exactly SKILL.md, SKILL-MECHANICS.md, LICENSE and NOTICE", () => {
  assert.deepEqual(readdirSync(dir).sort(), ["LICENSE", "NOTICE", "SKILL-MECHANICS.md", "SKILL.md"]);
});

test("LICENSE is upstream's MIT text and NOTICE names the upstream and its base commit", () => {
  const license = readFileSync(dir + "LICENSE", "utf8");
  const notice = readFileSync(dir + "NOTICE", "utf8");
  assert.ok(license.startsWith("MIT License"));
  assert.ok(license.includes("Copyright (c) 2026 Matt Pocock"));
  assert.ok(notice.includes("4ae2317ddb22b046e26455e4f9c9b3aaaf62212d"));
  assert.ok(notice.includes("https://github.com/mattpocock/skills"));
});

test("every relative Markdown link of the two skill files resolves inside the copy", () => {
  for (const file of ["SKILL.md", "SKILL-MECHANICS.md"]) {
    const text = readFileSync(dir + file, "utf8");
    const targets = [...text.matchAll(/\]\(([^)\s]+)\)/g)]
      .map((m) => m[1] as string)
      .filter((t) => !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(t) && !t.startsWith("#") && !t.startsWith("/"))
      .map((t) => t.split("#")[0] as string);
    assert.ok(targets.length > 0, `${file} has no relative link`);
    for (const target of targets) {
      assert.ok(existsSync(dir + target), `${file} links to missing ${target}`);
    }
  }
});
