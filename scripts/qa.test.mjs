import "../tools/repo-qa/scripts/qa.test.mjs";
import "../tools/repo-qa/scripts/portability.test.mjs";
import "./qa-wake.test.mjs";

import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { MARKER, REPOS, batchMarker, day, run, status } from "./qa.mjs";

test("the CPD entry point preserves historical attribution and the legacy batch variable", () => {
  assert.equal(MARKER, "<!-- cpd-repo-qa:v1 -->");
  assert.deepEqual(REPOS, ["clayton-duarte/cpd", "clayton-duarte/dotfiles", "clayton-duarte/skills"]);
  const usageDir = mkdtempSync(join(tmpdir(), "cpd-qa-usage-test-"));
  const usageFile = join(usageDir, "usage.json");
  // The official reset can fall on any weekday; detailed pacing boundaries live upstream.
  const reset = new Date(Date.now() + 3600_000);
  writeFileSync(usageFile, JSON.stringify({ version: 1, source: "codex-app-server", sampled_at: new Date().toISOString(),
    limit_id: "codex", window_minutes: 10080, remaining_percent: 100,
    resets_at: reset.toISOString(), ordinary_usage_allowed: true, spend_control_reached: false }));
  const previous = { usage: process.env.QA_USAGE_FILE, legacy: process.env.CPD_QA_BATCH, generic: process.env.QA_BATCH };
  const batch = `${day(new Date())}-compat`;
  const descriptor = {version:1,id:"compat",started_at:new Date().toISOString(),limits:null,authorization:"Synthetic test approval"};
  try {
    process.env.QA_USAGE_FILE = usageFile;
    delete process.env.QA_BATCH;
    process.env.CPD_QA_BATCH = batch;
    const s = status(() => [{ number: 1, created_at: new Date().toISOString(), body: `${MARKER}\n${batchMarker(batch)}` }]);
    assert.deepEqual(s.counts, { issues: 3, prs: 0 });
    assert.equal(s.batch, batch);
    let posts = 0;
    const result = run(["pr", REPOS[0], "qa/fix", "literal", "body.md"], (args, payload) => {
      if (!args.includes("POST")) return [];
      posts++;
      assert.equal(payload.base, "main");
      assert.equal(payload.body, `${MARKER}\n${batchMarker(batch)}\n<!-- cpd-repo-qa-run:compat -->\n\nreason`);
      return { number: 1 };
    }, () => "reason", undefined, descriptor);
    assert.equal(result.exit, 0);
    assert.equal(posts, 1);
    process.env.QA_BATCH = `${day(new Date())}-different`;
    assert.throws(() => status(() => assert.fail("Conflicting batch variables must not reach GitHub")), /Conflicting/);
  } finally {
    for (const [key, value] of [["QA_USAGE_FILE", previous.usage], ["CPD_QA_BATCH", previous.legacy], ["QA_BATCH", previous.generic]]) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
    rmSync(usageDir, { recursive: true, force: true });
  }
});

test("the vendored QA package matches its immutable upstream snapshot", async () => {
  const { createHash } = await import("node:crypto");
  const { readFileSync, readdirSync } = await import("node:fs");
  const { fileURLToPath } = await import("node:url");
  const { join } = await import("node:path");
  const pin = JSON.parse(readFileSync(new URL("../tools/repo-qa.upstream.json", import.meta.url), "utf8"));
  assert.equal(pin.version, 1);
  assert.equal(pin.repository, "https://github.com/clayton-duarte/skills");
  assert.match(pin.revision, /^[0-9a-f]{40}$/);
  assert.equal(pin.path, "skills/repo-qa");
  const root = fileURLToPath(new URL("../tools/repo-qa/", import.meta.url));
  const files = {};
  const visit = (dir, prefix = "") => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const relative = `${prefix}${entry.name}`;
      if (entry.isDirectory()) visit(join(dir, entry.name), `${relative}/`);
      else {
        assert.ok(entry.isFile(), `Unexpected non-file in vendored snapshot: ${relative}`);
        files[relative] = createHash("sha256").update(readFileSync(join(dir, entry.name))).digest("hex");
      }
    }
  };
  visit(root);
  assert.deepEqual(files, pin.files, "Update the source skill first, then refresh the complete pinned snapshot");
});
