import { test } from "node:test";
import assert from "node:assert/strict";
import { run as prRebase } from "../../src/commands/pr-rebase.ts";
import { gitOut, rebaseInProgress, statusPorcelain } from "../../src/git/git.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { commitFile } from "../git/fixtures.ts";
import { dispatchedWorkflow, sandboxCpd, withFakeWt } from "../pr/setup.ts";

sandboxCpd();

const lines = (a: string) => `line1\n${a}\nline3\nline4\nline5\n`;
const remoteHead = (f: { repo: string; origin: string }, wf: string) => gitOut(f.repo, ["ls-remote", f.origin, `refs/heads/cpd/${wf}`]).then((s) => s.split(/\s/)[0]);

test("clean: the Lead Branch is rebased onto the advanced base and force-pushed", async () => {
  const f = await dispatchedWorkflow("rb-clean", { jobs: ["a"] });
  await commitFile(f.leadWorktree, "lead.txt", "l\n", "lead: add lead.txt");
  await gitOut(f.leadWorktree, ["push", "-u", "origin", "cpd/rb-clean"]);
  const mainTip = await commitFile(f.repo, "main1.txt", "m\n", "main: add main1");

  const r: any = await prRebase(["--wf", f.wf]);
  assert.equal(r.ok, true);
  await gitOut(f.repo, ["merge-base", "--is-ancestor", mainTip, "cpd/rb-clean"]);
  const head = await gitOut(f.leadWorktree, ["rev-parse", "HEAD"]);
  assert.equal(await remoteHead(f, "rb-clean"), head);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
});

test("base not moved: a no-op rebase, the push still happens", async () => {
  const f = await dispatchedWorkflow("rb-noop", { jobs: ["a"] });
  await commitFile(f.leadWorktree, "lead.txt", "l\n", "lead: add lead.txt");
  const r: any = await prRebase(["--wf", f.wf]);
  assert.equal(r.ok, true);
  assert.equal(await remoteHead(f, "rb-noop"), await gitOut(f.leadWorktree, ["rev-parse", "HEAD"]));
});

test("conflict: rebase_conflict (exit 3) with the files; the Lead worktree and origin are unchanged", async () => {
  const f = await dispatchedWorkflow("rb-conflict", { jobs: ["a"] });
  await commitFile(f.leadWorktree, "app.txt", lines("lead"), "lead: edit line 2");
  await gitOut(f.leadWorktree, ["push", "-u", "origin", "cpd/rb-conflict"]);
  await commitFile(f.repo, "app.txt", lines("main"), "main: edit line 2");
  const before = await gitOut(f.leadWorktree, ["rev-parse", "HEAD"]);
  const pushed = await remoteHead(f, "rb-conflict");

  await assert.rejects(prRebase(["--wf", f.wf]), (e: unknown) => {
    assert.ok(e instanceof CpdError);
    assert.deepEqual([e.code, e.exitCode, e.details.files], ["rebase_conflict", 3, ["app.txt"]]);
    return true;
  });
  assert.equal(await gitOut(f.leadWorktree, ["rev-parse", "HEAD"]), before);
  assert.equal(await rebaseInProgress(f.leadWorktree), false);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
  assert.equal(await remoteHead(f, "rb-conflict"), pushed);
});

test("Gate failure after the rebase: gate_failed (exit 3), the Gate ran after the rebase, nothing is pushed", async () => {
  const f = await dispatchedWorkflow("rb-gate", { jobs: ["a"] });
  await commitFile(f.leadWorktree, "lead.txt", "l\n", "lead: add lead.txt");
  await gitOut(f.leadWorktree, ["push", "-u", "origin", "cpd/rb-gate"]);
  const pushed = await remoteHead(f, "rb-gate");
  const mainTip = await commitFile(f.repo, "main1.txt", "m\n", "main: add main1");

  const { result, calls } = await withFakeWt(64, "gate says no", () => prRebase(["--wf", f.wf]));
  assert.equal(result.status, "rejected");
  const e = (result as PromiseRejectedResult).reason as CpdError;
  assert.deepEqual([e.code, e.exitCode, e.details.outputTail], ["gate_failed", 3, "gate says no"]);
  assert.deepEqual(calls, [["--yes", "-C", f.leadWorktree, "hook", "pre-merge"]]);
  assert.equal(await remoteHead(f, "rb-gate"), pushed, "origin is unchanged");
  await gitOut(f.repo, ["merge-base", "--is-ancestor", mainTip, "cpd/rb-gate"]); // the rebase itself happened before the Gate
});

test("Gate error (any other exit) after the rebase: gate_error, nothing is pushed", async () => {
  const f = await dispatchedWorkflow("rb-gate-err", { jobs: ["a"] });
  await commitFile(f.leadWorktree, "lead.txt", "l\n", "lead: add lead.txt");
  await gitOut(f.leadWorktree, ["push", "-u", "origin", "cpd/rb-gate-err"]);
  const pushed = await remoteHead(f, "rb-gate-err");
  await commitFile(f.repo, "main1.txt", "m\n", "main: add main1");
  const { result } = await withFakeWt(42, "boom", () => prRebase(["--wf", f.wf]));
  assert.equal(((result as PromiseRejectedResult).reason as CpdError).code, "gate_error");
  assert.equal(await remoteHead(f, "rb-gate-err"), pushed);
});
