import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { tooLong, unseen, unseenReason } from "../../lib/askguard.ts";
import { reasonText as hostReason } from "../../lib/hostapprove.ts";
import { reasonText as mainReason } from "../../lib/mainguard.ts";
import { reasonText as secretReason } from "../../lib/secretguard.ts";
import { hostVerdict, type Where } from "../../durable-host/guards.ts";

/** A primary checkout (a `.git` directory), a plain directory and the visible chat the G4 guard reads; no git runs. */
function fixtures(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "durable-host-guards-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(primary, "docs"));
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  const where: Where = { cwd: plain, home: tmp, visible: [] };
  return { tmp, primary, plain, where };
}

test("hostVerdict refuses an edit inside a primary checkout", (t) => {
  const x = fixtures(t);
  const abs = join(x.primary, "docs", "commands.md");
  for (const name of ["edit", "write"]) {
    assert.deepEqual(hostVerdict({ id: "c", name, arguments: { path: abs } }, x.where, "lead"), { block: mainReason(`${name} ${abs} in ${x.primary}`) }, name);
  }
  const f = join(x.primary, "f");
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: `echo x > ${f}` } }, x.where, "lead"), { block: mainReason(`> ${f} in ${x.primary}`) });
  assert.equal(hostVerdict({ id: "c", name: "write", arguments: { path: join(x.plain, "f") } }, x.where, "lead"), undefined);
});

test("hostVerdict refuses raw cpd secret approve and raw cpd host approve in bash", (t) => {
  const x = fixtures(t);
  assert.deepEqual(
    hostVerdict({ id: "c", name: "bash", arguments: { command: "cpd secret approve --action get --vault v --item i --field f" } }, x.where, "lead"),
    { block: secretReason("cpd secret approve") },
  );
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: "cpd host approve --reason dirty" } }, x.where, "lead"), { block: hostReason("cpd host approve") });
});

test("hostVerdict does not apply the unfiltered test guard", (t) => {
  const x = fixtures(t);
  for (const role of ["lead", "manager"] as const) {
    for (const command of ["cargo test --workspace", "cargo test", "npm test"]) {
      // no test-guard block: a Lead's or Manager's plain bash is only rewritten to carry the 30 s timeout (noblock)
      assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command } }, x.where, role), { arguments: { command, timeout: 30 } }, `${role}: ${command}`);
    }
  }
});

test("hostVerdict does not apply the explore profile", (t) => {
  const x = fixtures(t);
  // an explore Worker could not write outside its artefact; a Lead may write anywhere outside a primary checkout.
  assert.equal(hostVerdict({ id: "c", name: "write", arguments: { path: join(x.plain, "notes.md") } }, x.where, "lead"), undefined);
  // a bash with no timeout is rewritten to carry the 30 s timeout (noblock), not blocked by an explore profile
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: "echo x > notes.md" } }, x.where, "lead"), { arguments: { command: "echo x > notes.md", timeout: 30 } });
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: "ls -la" } }, x.where, "lead"), { arguments: { command: "ls -la", timeout: 30 } });
});

// The ask guards of contract §6.16 pinned outside the named tests too (tooLong, unseen).
test("hostVerdict refuses a too-long ask_user_question with the lib's text", (t) => {
  const x = fixtures(t);
  const args = { questions: [{ question: "x".repeat(1501) }] };
  assert.deepEqual(hostVerdict({ id: "c", name: "ask_user_question", arguments: args }, x.where, "lead"), { block: tooLong(args) });
});

test("hostVerdict refuses an ask_user_question whose range cites an unseen label", (t) => {
  const x = fixtures(t);
  const args = { questions: [{ question: "B2-B4?" }] };
  assert.equal(unseen(args, "c", x.where.visible), unseenReason(["B3"]));
  assert.deepEqual(hostVerdict({ id: "c", name: "ask_user_question", arguments: args }, x.where, "manager"), { block: unseenReason(["B3"]) });
});
