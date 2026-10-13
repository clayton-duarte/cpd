import assert from "node:assert/strict";
import { test } from "node:test";
import { gitOut, rebaseInProgress, statusPorcelain } from "../../src/git/git.ts";
import { rebaseLeadOntoBase } from "../../src/git/pr-rebase.ts";
import { W1, commitFile, createFixture, head, isLinear, log, sandbox, write } from "./fixtures.ts";

sandbox();

const lines = (a: string) => `line1\n${a}\nline3\nline4\nline5\n`;

test("clean case: the Lead is rebased onto the advanced base, history linear, authors kept", async () => {
  const f = await createFixture();
  await commitFile(f.leadWorktree, "app.txt", lines("lead"), "lead: edit line 2", W1);
  await commitFile(f.leadWorktree, "lead2.txt", "x\n", "lead: add lead2", W1);
  const oldLead = await head(f.leadWorktree);
  const mainTip = await commitFile(f.repo, "main1.txt", "m\n", "main: add main1");

  const r = await rebaseLeadOntoBase({ leadWorktree: f.leadWorktree, leadBranch: f.leadBranch, base: "main" });
  assert.deepEqual(r, { result: "rebased" });
  assert.equal(await gitOut(f.repo, ["merge-base", "--is-ancestor", mainTip, f.leadBranch]).then(() => true), true);
  assert.equal(await log(f.leadWorktree, `main..${f.leadBranch}`), "Worker One|lead: add lead2\nWorker One|lead: edit line 2");
  assert.equal(await isLinear(f.leadWorktree, f.leadBranch), true);
  assert.notEqual(await head(f.leadWorktree), oldLead);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
  assert.equal(await rebaseInProgress(f.leadWorktree), false);
});

test("conflict: files are returned, the Lead is unchanged and not mid-rebase", async () => {
  const f = await createFixture();
  await commitFile(f.leadWorktree, "app.txt", lines("lead"), "lead: edit line 2");
  await commitFile(f.repo, "app.txt", lines("main"), "main: edit line 2");
  const before = await head(f.leadWorktree);

  const r = await rebaseLeadOntoBase({ leadWorktree: f.leadWorktree, leadBranch: f.leadBranch, base: "main" });
  assert.deepEqual(r, { result: "conflict", files: ["app.txt"] });
  assert.equal(await head(f.leadWorktree), before);
  assert.equal(await rebaseInProgress(f.leadWorktree), false);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
});

test("a commit-by-commit conflict that merge-tree cannot see is aborted: Lead unchanged, not mid-rebase", async () => {
  const f = await createFixture();
  // Net change on the Lead is nil, so `git merge-tree` is clean; replaying commit 1 alone conflicts.
  await commitFile(f.leadWorktree, "app.txt", lines("lead"), "lead: edit line 2");
  await commitFile(f.leadWorktree, "app.txt", lines("line2"), "lead: revert line 2");
  await commitFile(f.repo, "app.txt", lines("main"), "main: edit line 2");
  const before = await head(f.leadWorktree);

  const r = await rebaseLeadOntoBase({ leadWorktree: f.leadWorktree, leadBranch: f.leadBranch, base: "main" });
  assert.deepEqual(r, { result: "conflict", files: ["app.txt"] });
  assert.equal(await head(f.leadWorktree), before);
  assert.equal(await rebaseInProgress(f.leadWorktree), false);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
});

test("a dirty Lead worktree is refused", async () => {
  const f = await createFixture();
  await write(f.leadWorktree, "README.md", "dirty\n");
  await assert.rejects(
    rebaseLeadOntoBase({ leadWorktree: f.leadWorktree, leadBranch: f.leadBranch, base: "main" }),
    (e: any) => e.code === "dirty_lead" && e.exitCode === 3,
  );
});
