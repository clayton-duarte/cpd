import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { reasonText as hostReason } from "../../lib/hostapprove.ts";
import { reasonText as mainReason } from "../../lib/mainguard.ts";
import { reasonText as secretReason } from "../../lib/secretguard.ts";
import { hostVerdict } from "../../durable-host/guards.ts";

/** A primary checkout (a `.git` directory) and a plain directory; no git runs. */
function fixtures(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "durable-host-guards-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(primary, "docs"));
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  return { tmp, primary, plain, where: { cwd: plain, home: tmp } };
}

test("hostVerdict refuses an edit inside a primary checkout", (t) => {
  const x = fixtures(t);
  const abs = join(x.primary, "docs", "commands.md");
  for (const name of ["edit", "write"]) {
    assert.deepEqual(hostVerdict({ name, arguments: { path: abs } }, x.where), { block: mainReason(`${name} ${abs} in ${x.primary}`) }, name);
  }
  const f = join(x.primary, "f");
  assert.deepEqual(hostVerdict({ name: "bash", arguments: { command: `echo x > ${f}` } }, x.where), { block: mainReason(`> ${f} in ${x.primary}`) });
  assert.equal(hostVerdict({ name: "write", arguments: { path: join(x.plain, "f") } }, x.where), undefined);
});

test("hostVerdict refuses raw cpd secret approve and raw cpd host approve in bash", (t) => {
  const x = fixtures(t);
  assert.deepEqual(
    hostVerdict({ name: "bash", arguments: { command: "cpd secret approve --action get --vault v --item i --field f" } }, x.where),
    { block: secretReason("cpd secret approve") },
  );
  assert.deepEqual(hostVerdict({ name: "bash", arguments: { command: "cpd host approve --reason dirty" } }, x.where), { block: hostReason("cpd host approve") });
});

test("hostVerdict does not apply the unfiltered test guard", (t) => {
  const x = fixtures(t);
  for (const command of ["cargo test --workspace", "cargo test", "npm test"]) {
    assert.equal(hostVerdict({ name: "bash", arguments: { command } }, x.where), undefined, command);
  }
});

test("hostVerdict does not apply the explore profile", (t) => {
  const x = fixtures(t);
  // an explore Worker could not write outside its artefact; a Lead may write anywhere outside a primary checkout.
  assert.equal(hostVerdict({ name: "write", arguments: { path: join(x.plain, "notes.md") } }, x.where), undefined);
  assert.equal(hostVerdict({ name: "bash", arguments: { command: "echo x > notes.md" } }, x.where), undefined);
  assert.equal(hostVerdict({ name: "bash", arguments: { command: "ls -la" } }, x.where), undefined);
});
