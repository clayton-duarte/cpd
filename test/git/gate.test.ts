import assert from "node:assert/strict";
import { test } from "node:test";
import { commitsAhead, runGate, tamperCheck, treeClean } from "../../src/git/gate.ts";
import { gitOut } from "../../src/git/git.ts";
import { commitFile, createFixture, makeAttempt, sandbox, write } from "./fixtures.ts";

sandbox();

test("tamperCheck returns exactly the forbidden paths touched", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j1-a1", [
    { file: "app.txt", content: "changed\n", message: "ok change" },
    { file: "verify.sh", content: "#!/bin/sh\nexit 0\n", message: "weaken verify" },
    { file: ".cpd/notes.md", content: "x\n", message: "cpd dir" },
    { file: ".config/wt.toml", content: "[pre-merge]\n", message: "weaken config" },
  ]);
  assert.deepEqual((await tamperCheck(a.path, f.leadBranch, a.branch)).sort(), [".config/wt.toml", ".cpd/notes.md", "verify.sh"]);
});

// git diff detects renames by default and --name-only prints only the new name, so a rename must not hide the old path.
test.describe("tamperCheck sees a forbidden file renamed away (Gate bypass, t3 F11)", () => {
  for (const [from, to] of [
    [".config/wt.toml", ".config/wt.toml.bak"],
    ["verify.sh", "verify.old"],
  ] as const) {
    test(`git mv ${from} ${to}`, async () => {
      const f = await createFixture();
      const a = await makeAttempt(f, "j1-a1");
      await gitOut(a.path, ["mv", from, to]);
      await gitOut(a.path, ["commit", "-q", "-m", `rename ${from}`]);
      assert.deepEqual(await tamperCheck(a.path, f.leadBranch, a.branch), [from]);
    });
  }

  test("git mv of a file under .cpd/", async () => {
    const f = await createFixture();
    await commitFile(f.leadWorktree, ".cpd/notes.md", "x\n", "lead: add cpd notes");
    const a = await makeAttempt(f, "j1-a1");
    await gitOut(a.path, ["mv", ".cpd/notes.md", "notes.md"]);
    await gitOut(a.path, ["commit", "-q", "-m", "move cpd notes out"]);
    assert.deepEqual(await tamperCheck(a.path, f.leadBranch, a.branch), [".cpd/notes.md"]);
  });
});

test("tamperCheck is empty for an honest Attempt", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j1-a1", [{ file: "src/x.txt", content: "x\n", message: "add x" }]);
  assert.deepEqual(await tamperCheck(a.path, f.leadBranch, a.branch), []);
});

test("runGate: pass, gate failure (64) with output, and gate_error for any other exit code", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j1-a1");
  const ok = await runGate(a.path);
  assert.deepEqual({ passed: ok.passed, exitCode: ok.exitCode }, { passed: true, exitCode: 0 });
  assert.match(ok.outputTail, /verify: ok/);

  await write(a.path, "FAIL_GATE", "");
  const bad = await runGate(a.path);
  assert.deepEqual({ passed: bad.passed, exitCode: bad.exitCode }, { passed: false, exitCode: 64 });
  assert.match(bad.outputTail, /FAIL_GATE present/);

  // A Gate config that does not use the 64 wrapper: wt propagates the hook's own exit code.
  await write(a.path, ".config/wt.toml", `[pre-merge]\nverify = "exit 42"\n`);
  await assert.rejects(runGate(a.path), (e: any) => e.code === "gate_error" && e.details.code === 42);
});

test("treeClean ignores ignored files and sees untracked ones; commitsAhead counts commits past base", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j1-a1");
  assert.equal(await treeClean(a.path), true);
  assert.equal(await commitsAhead(a.path, f.leadBranch), 0);
  await write(a.path, "FAIL_GATE", ""); // gitignored
  assert.equal(await treeClean(a.path), true);
  await write(a.path, "new.txt", "x");
  assert.equal(await treeClean(a.path), false);
  await commitFile(a.path, "new.txt", "x", "c1");
  await commitFile(a.path, "new2.txt", "y", "c2");
  assert.equal(await treeClean(a.path), true);
  assert.equal(await commitsAhead(a.path, f.leadBranch), 2);
});
