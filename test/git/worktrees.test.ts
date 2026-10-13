import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { createAttemptWorktree, removeAttemptWorktree } from "../../src/git/attempt-worktree.ts";
import { branchExists, git, gitOut, mergeBase, realpath } from "../../src/git/git.ts";
import { createLeadBranch } from "../../src/git/lead-branch.ts";
import { mergeAttempt } from "../../src/git/merge.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { attemptBranch, commitFile, createFixture, createRepo, head, makeAttempt, sandbox } from "./fixtures.ts";

sandbox();

test("createLeadBranch makes its own worktree and is idempotent", async () => {
  const repo = await createRepo();
  const a = await createLeadBranch(repo, "cpd/demo", "main");
  assert.notEqual(a.path, realpath(repo));
  assert.equal(await gitOut(a.path, ["rev-parse", "--abbrev-ref", "HEAD"]), "cpd/demo");
  assert.equal(await gitOut(repo, ["rev-parse", "--abbrev-ref", "HEAD"]), "main");
  const b = await createLeadBranch(repo, "cpd/demo", "main");
  assert.equal(b.path, a.path);
});

test("createAttemptWorktree blocks until setup finished and the merge-base is the Lead HEAD", async () => {
  const f = await createFixture();
  await commitFile(f.leadWorktree, "lead.txt", "lead\n", "lead: add lead.txt");
  const before = Date.now();
  const { path } = await createAttemptWorktree(f.leadWorktree, attemptBranch(f, "j1-a1"), f.leadBranch);
  const after = Date.now();
  const doneAt = Number(readFileSync(join(path, ".setup-done"), "utf8"));
  assert.ok(doneAt >= before && doneAt <= after, "setup finished inside the call");
  assert.ok(after - before >= 600, "the call blocked on the pre-start sleep");
  assert.equal(await mergeBase(path, f.leadBranch, attemptBranch(f, "j1-a1")), await head(f.leadWorktree));
  assert.equal(existsSync(join(path, "lead.txt")), true);
});

test("a failing pre-start throws setup_failed and leaves no worktree or branch", async () => {
  const f = await createFixture();
  // pre-start runs from the Attempt branch's own config, so break it on the Lead Branch first.
  await commitFile(f.leadWorktree, ".config/wt.toml", `[pre-start]\nsetup = "echo boom-setup >&2; exit 3"\n`, "lead: break setup");
  const branch = attemptBranch(f, "j1-a1");
  const err = await createAttemptWorktree(f.leadWorktree, branch, f.leadBranch).then(
    () => assert.fail("expected setup_failed"),
    (e: unknown) => e as CpdError,
  );
  assert.equal(err.code, "setup_failed");
  assert.equal(err.details.exitCode, 3);
  assert.match(String(err.details.stderrTail), /boom-setup/);
  assert.equal(await branchExists(f.repo, branch), false);
  const wts = await gitOut(f.repo, ["worktree", "list", "--porcelain"]);
  assert.doesNotMatch(wts, /j1-a1/);
});

test("createAttemptWorktree refuses to reuse an existing branch and leaves it alone", async () => {
  const f = await createFixture();
  const branch = attemptBranch(f, "j1-a1");
  await gitOut(f.repo, ["branch", branch, f.leadBranch]);
  const err = await createAttemptWorktree(f.leadWorktree, branch, f.leadBranch).then(
    () => assert.fail("expected refusal"),
    (e: unknown) => e as CpdError,
  );
  assert.equal(err.code, "attempt_branch_exists");
  assert.equal(await branchExists(f.repo, branch), true);
});

test("removeAttemptWorktree deletes the worktree and branch of a merged Attempt, and is idempotent", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j1-a1", [{ file: "j1.txt", content: "j1\n", message: "j1: add" }]);
  const m = await mergeAttempt({ repo: f.repo, leadWorktree: f.leadWorktree, leadBranch: f.leadBranch, attemptWorktree: a.path, branch: a.branch });
  assert.equal(m.result, "merged");
  await removeAttemptWorktree(f.repo, f.leadWorktree, a.branch);
  assert.equal(existsSync(a.path), false);
  assert.equal(await branchExists(f.repo, a.branch), false);
  await removeAttemptWorktree(f.repo, f.leadWorktree, a.branch);
});

test("removeAttemptWorktree keeps an unmerged Attempt's branch (guarded delete)", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j1-a1", [{ file: "j1.txt", content: "j1\n", message: "j1: add" }]);
  const err = await removeAttemptWorktree(f.repo, f.leadWorktree, a.branch).then(
    () => assert.fail("expected refusal"),
    (e: unknown) => e as CpdError,
  );
  assert.equal(err.code, "branch_not_merged");
  assert.equal(await branchExists(f.repo, a.branch), true);
  const r = await git(f.repo, ["worktree", "list", "--porcelain"]);
  assert.doesNotMatch(r.stdout, /j1-a1/, "the worktree itself was removed");
});
