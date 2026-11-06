import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { reasonText as electronReason } from "../../lib/electronguard.ts";
import { reasonText as hostReason } from "../../lib/hostapprove.ts";
import { reasonText as mainReason } from "../../lib/mainguard.ts";
import { reasonText as secretReason } from "../../lib/secretguard.ts";
import { reasonText as testReason } from "../../lib/testguard.ts";
import { workerVerdict } from "../../durable-worker/guards.ts";

// The Worker rows of the adapter's `tool_call` handler (contract §6.12, guards.ts); inputs are those of the guards' own tests.

/** A primary checkout (a `.git` directory) and a plain directory; no git runs. */
function fixtures(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "durable-guards-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(primary, "docs"));
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  return { tmp, primary, plain };
}

test("workerVerdict blocks an edit or write whose path is in a primary checkout", (t) => {
  const x = fixtures(t);
  const abs = join(x.primary, "docs", "commands.md");
  for (const name of ["edit", "write"]) {
    assert.deepEqual(workerVerdict({ name, arguments: { path: abs } }, { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") }), { block: mainReason(`${name} ${abs} in ${x.primary}`) }, name);
  }
});

test("workerVerdict blocks a bash command that moves a primary checkout", (t) => {
  const x = fixtures(t);
  const f = join(x.primary, "f");
  assert.deepEqual(workerVerdict({ name: "bash", arguments: { command: `echo x > ${f}` } }, { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") }), { block: mainReason(`> ${f} in ${x.primary}`) });
});

test("workerVerdict blocks a bash command that bypasses the secret rules", (t) => {
  const x = fixtures(t);
  const where = { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") };
  assert.deepEqual(workerVerdict({ name: "bash", arguments: { command: "op read op://cpd/x/y" } }, where), { block: secretReason("op read") });
  assert.deepEqual(
    workerVerdict({ name: "bash", arguments: { command: "cpd secret approve --action get --vault v --item i --field f" } }, where),
    { block: secretReason("cpd secret approve") },
  );
});

test("workerVerdict blocks a bash command that bypasses host approval", (t) => {
  const x = fixtures(t);
  assert.deepEqual(workerVerdict({ name: "bash", arguments: { command: "cpd host approve --reason dirty" } }, { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") }), { block: hostReason("cpd host approve") });
});

test("workerVerdict blocks an unfiltered full test run", (t) => {
  const x = fixtures(t);
  const where = { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") };
  assert.deepEqual(workerVerdict({ name: "bash", arguments: { command: "cargo test --workspace" } }, where), { block: testReason("cargo test --workspace") });
});

test("workerVerdict lets an ordinary call through", (t) => {
  const x = fixtures(t);
  const where = { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") };
  assert.equal(workerVerdict({ name: "bash", arguments: { command: "ls -la" } }, where), undefined);
  assert.equal(workerVerdict({ name: "bash", arguments: { command: 7 } }, where), undefined);
  assert.equal(workerVerdict({ name: "read", arguments: { path: join(x.primary, "f") } }, where), undefined);
  assert.equal(workerVerdict({ name: "write", arguments: { path: join(x.plain, "f") } }, where), undefined);
});

test("workerVerdict gives the reason text of the guard that matched", (t) => {
  const x = fixtures(t);
  const where = { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") };
  const reasons = [
    workerVerdict({ name: "edit", arguments: { path: join(x.primary, "f") } }, where),
    workerVerdict({ name: "bash", arguments: { command: "op whoami" } }, where),
    workerVerdict({ name: "bash", arguments: { command: "cpd host approve" } }, where),
    workerVerdict({ name: "bash", arguments: { command: "cargo test" } }, where),
  ].map((v) => v?.block);
  assert.ok(reasons[0]?.includes("primary checkout"), String(reasons[0]));
  assert.ok(reasons[1]?.includes("1Password"), String(reasons[1]));
  assert.ok(reasons[2]?.includes("cpd_host_approve"), String(reasons[2]));
  assert.ok(reasons[3]?.includes("the full suite is the Gate's"), String(reasons[3]));
});

test("workerVerdict blocks a raw electron invocation in bash", (t) => {
  const x = fixtures(t);
  const where = { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "cpd") };
  assert.deepEqual(workerVerdict({ name: "bash", arguments: { command: "npx electron" } }, where), { block: electronReason("npx electron") });
});
