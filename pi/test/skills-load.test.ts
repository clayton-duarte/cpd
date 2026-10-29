// Contract §8.3: pi's own loader must load every skill of pi/skills without a diagnostic.
import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { loadSkillsFromDir } from "@earendil-works/pi-coding-agent";

const loaded = loadSkillsFromDir({ dir: fileURLToPath(new URL("../skills/", import.meta.url)), source: "test" });

test("pi loads cpd-lead and cpd-manager from pi/skills", () => {
  assert.deepEqual(loaded.skills.map((s) => s.name).sort(), ["cpd-lead", "cpd-manager"]);
});

test("pi reports no diagnostic for the skills of pi/skills", () => {
  assert.deepEqual(loaded.diagnostics.map((d) => d.message), []);
});
