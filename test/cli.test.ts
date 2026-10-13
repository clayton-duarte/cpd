import { test } from "node:test";
import assert from "node:assert/strict";
import { symlinkSync } from "node:fs";
import { join } from "node:path";
import { CLI, runCli, tmpDir } from "./helpers/index.ts";

// Commands listed in contract §11.
const EXPECTED = [
  "doctor", "tiers", "lead start", "lead attach",
  "workflow init", "workflow show", "workflow phase",
  "job add", "job edit", "job list", "job show", "job cancel", "job retry", "job dispatch",
  "plan render", "plan decision", "plan lock", "watch",
  "attempt run", "attempt submit", "attempt read", "attempt list", "attempt show",
  "review decide", "ask", "answer", "pr open", "pr poll", "pr rebase",
];

test("--help lists every contract command", async () => {
  const r = await runCli(["--help"]);
  assert.equal(r.code, 0);
  assert.equal(r.json.ok, true);
  assert.deepEqual([...r.json.commands].sort(), [...EXPECTED].sort());
});

for (const cmd of EXPECTED) {
  test(`stub: cpd ${cmd} -> not_implemented`, async () => {
    const r = await runCli(cmd.split(" "));
    assert.equal(r.code, 1);
    assert.equal(r.json.ok, false);
    assert.equal(r.json.error.code, "not_implemented");
    assert.ok(r.json.error.message.includes(cmd));
  });
}

test("unknown command exits 2", async () => {
  for (const args of [["nope"], ["job", "nope"], ["job"]]) {
    const r = await runCli(args);
    assert.equal(r.code, 2);
    assert.equal(r.json.error.code, "unknown_command");
  }
});

test("--version", async () => {
  const r = await runCli(["--version"]);
  assert.equal(r.code, 0);
  assert.equal(r.json.ok, true);
  assert.equal(typeof r.json.version, "string");
});

test("works through a symlink", async () => {
  const link = join(tmpDir(), "cpd");
  symlinkSync(CLI, link);
  const r = await runCli(["--version"], { bin: link });
  assert.equal(r.code, 0);
  assert.equal(r.json.ok, true);
});
