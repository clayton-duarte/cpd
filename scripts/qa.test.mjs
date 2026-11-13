import "../tools/repo-qa/scripts/qa.test.mjs";
import "../tools/repo-qa/scripts/portability.test.mjs";

import assert from "node:assert/strict";
import { test } from "node:test";
import { MARKER, REPOS, batchMarker, day, run, status } from "./qa.mjs";

test("the CPD entry point preserves historical attribution and the legacy batch variable", () => {
  assert.equal(MARKER, "<!-- cpd-repo-qa:v1 -->");
  assert.deepEqual(REPOS, ["clayton-duarte/cpd", "clayton-duarte/dotfiles"]);
  const previous = { legacy: process.env.CPD_QA_BATCH, generic: process.env.QA_BATCH };
  const batch = `${day(new Date())}-compat`;
  try {
    delete process.env.QA_BATCH;
    process.env.CPD_QA_BATCH = batch;
    const s = status(() => [{ number: 1, created_at: new Date().toISOString(), body: `${MARKER}\n${batchMarker(batch)}` }]);
    assert.deepEqual(s.counts, { issues: 2, prs: 0 });
    assert.equal(s.batch, batch);
    let posts = 0;
    const result = run(["pr", REPOS[0], "qa/fix", "literal", "body.md"], (args, payload) => {
      if (!args.includes("POST")) return [];
      posts++;
      assert.equal(payload.base, "main");
      assert.equal(payload.body, `${MARKER}\n${batchMarker(batch)}\n\nreason`);
      return { number: 1 };
    }, () => "reason");
    assert.equal(result.exit, 0);
    assert.equal(posts, 1);
    process.env.QA_BATCH = `${day(new Date())}-different`;
    assert.throws(() => status(() => assert.fail("Conflicting batch variables must not reach GitHub")), /Conflicting/);
  } finally {
    for (const [key, value] of [["CPD_QA_BATCH", previous.legacy], ["QA_BATCH", previous.generic]]) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
  }
});
