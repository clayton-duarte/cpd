import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

test("a copied kit works in another repository and cwd, resumes a run and refuses extra writes", () => {
  const dir = mkdtempSync(join(tmpdir(), "repo-qa-portability-"));
  try {
    const repo = join(dir, "unrelated repo");
    const bin = join(dir, "bin");
    const elsewhere = join(dir, "elsewhere");
    for (const path of [repo, bin, elsewhere]) mkdirSync(path);
    cpSync(new URL("../", import.meta.url), join(repo, "repo-qa"), { recursive: true });
    const configFile = join(repo, "qa.config.json");
    writeFileSync(configFile, JSON.stringify({ version: 1, scope: "other-repo-qa", timezone: "Europe/London",
      limits: { issues: 2, prs: 2 }, repositories: [{ name: "acme/unrelated", base: "trunk" }] }));
    const runFile = join(repo, "run.json");
    writeFileSync(runFile, JSON.stringify({ version: 1, id: "cold-start", started_at: new Date(Date.now() - 60_000).toISOString(),
      limits: { issues: 1, prs: 1 }, authorization: "User-approved cold-start experiment" }));
    const rows = join(dir, "github-rows.json");
    const calls = join(dir, "github-calls.jsonl");
    writeFileSync(rows, "[]");
    writeFileSync(calls, "");
    writeFileSync(join(bin, "gh"), `#!/usr/bin/env node
const fs = require("node:fs");
const args = process.argv.slice(2);
if (args[0] !== "api" || args[1] !== "--hostname" || args[2] !== "github.com") throw new Error("unexpected command");
if (!args[3].startsWith("repos/acme/unrelated/")) throw new Error("wrong repository");
const rows = JSON.parse(fs.readFileSync(process.env.QA_TEST_ROWS, "utf8"));
if (args.includes("POST")) {
  const payload = JSON.parse(fs.readFileSync(0, "utf8"));
  fs.appendFileSync(process.env.QA_TEST_CALLS, JSON.stringify(payload) + "\\n");
  const pr = args[3].endsWith("/pulls");
  const item = { number: rows.length + 1, body: payload.body, created_at: new Date().toISOString(), ...(pr ? { pull_request: {} } : {}) };
  rows.push(item);
  fs.writeFileSync(process.env.QA_TEST_ROWS, JSON.stringify(rows));
  console.log(JSON.stringify({ number: item.number, html_url: "https://example.test/" + item.number }));
} else console.log(JSON.stringify(rows));
`, { mode: 0o755 });
    const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, QA_TEST_ROWS: rows, QA_TEST_CALLS: calls };
    delete env.QA_BATCH;
    delete env.CPD_QA_BATCH;
    const script = join(repo, "repo-qa/scripts/qa.mjs");
    const invoke = (args) => spawnSync(process.execPath, [script, "--config", configFile, "--run", runFile, ...args], { cwd: elsewhere, env, encoding: "utf8" });
    let result = invoke(["status"]);
    assert.equal(result.status, 0, result.stderr + result.stdout + String(result.error ?? ""));
    assert.deepEqual(JSON.parse(result.stdout).run.counts, { issues: 0, prs: 0 });
    const bodyFile = join(repo, "literal body.md");
    writeFileSync(bodyFile, "literal $(command) `text`\nresearch");
    result = invoke(["issue", "acme/unrelated", "literal $(title)", bodyFile]);
    assert.equal(result.status, 0, result.stderr + result.stdout + String(result.error ?? ""));
    result = invoke(["status"]); // A fresh CLI process recovers counts only from GitHub.
    const status = JSON.parse(result.stdout);
    assert.equal(status.run.counts.issues, 1);
    assert.equal(status.can_create_issue, false);
    assert.equal(status.daily_remaining.issues, 1);
    assert.equal(invoke(["issue", "acme/unrelated", "extra", bodyFile]).status, 3);
    result = invoke(["pr", "acme/unrelated", "qa/fix", "fix", bodyFile]);
    assert.equal(result.status, 0, result.stderr + result.stdout + String(result.error ?? ""));
    assert.equal(invoke(["pr", "acme/unrelated", "qa/extra", "extra", bodyFile]).status, 3);
    const writes = readFileSync(calls, "utf8").trim().split("\n").map(JSON.parse);
    assert.equal(writes.length, 2);
    assert.equal(writes[0].title, "literal $(title)");
    assert.equal(writes[0].body, "<!-- other-repo-qa:v1 -->\n<!-- other-repo-qa-run:cold-start -->\n\nliteral $(command) `text`\nresearch");
    assert.equal(writes[1].base, "trunk");
    assert.equal(invoke(["issue", "unlisted/repo", "wrong", bodyFile]).status, 1);
    for (const invalid of ["{}", "null", "[]", "not JSON"]) {
      writeFileSync(runFile, invalid);
      assert.equal(invoke(["issue", "acme/unrelated", "invalid", bodyFile]).status, 1);
    }
    assert.equal(readFileSync(calls, "utf8").trim().split("\n").length, 2);
    assert.equal(spawnSync(process.execPath, [script, "--config", "missing.json", "status"], { cwd: elsewhere, env, encoding: "utf8" }).status, 1);
    const tests = spawnSync(process.execPath, [join(repo, "repo-qa/scripts/qa.test.mjs")], { cwd: elsewhere, env, encoding: "utf8" });
    assert.equal(tests.status, 0, tests.stderr + tests.stdout);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
