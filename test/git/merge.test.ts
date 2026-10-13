import assert from "node:assert/strict";
import { chmodSync, existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { removeAttemptWorktree } from "../../src/git/attempt-worktree.ts";
import { gitOut, rebaseInProgress, statusPorcelain } from "../../src/git/git.ts";
import { mergeAttempt, type MergeOutcome } from "../../src/git/merge.ts";
import { tmpDir } from "../helpers/index.ts";
import {
  W1, W2, commitFile, createFixture, head, isLinear, log, makeAttempt, sandbox, write, type Fixture,
} from "./fixtures.ts";

sandbox();

const merge = (f: Fixture, a: { path: string; branch: string }, extra: { casRetries?: number } = {}): Promise<MergeOutcome> =>
  mergeAttempt({ repo: f.repo, leadWorktree: f.leadWorktree, leadBranch: f.leadBranch, attemptWorktree: a.path, branch: a.branch, ...extra });

/** The Lead ref and checkout are exactly as before. */
async function assertLeadUntouched(f: Fixture, sha: string) {
  assert.equal(await head(f.leadWorktree), sha);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
}

test("two Attempts, the second needs a rebase: linear, original messages and authors, new SHAs, Lead clean", async () => {
  const f = await createFixture();
  const j2 = await makeAttempt(f, "j2-a1", [{ file: "j2.txt", content: "j2\n", message: "j2: add j2.txt", author: W2 }]);
  const j1 = await makeAttempt(f, "j1-a1", [
    { file: "j1.txt", content: "one\n", message: "j1: first commit", author: W1 },
    { file: "j1b.txt", content: "two\n", message: "j1: second commit", author: W1 },
  ]);
  const oldShas = (await log(j1.path, "HEAD", "%H")).split("\n");
  const base = await head(f.leadWorktree);

  const m2 = await merge(f, j2);
  assert.equal(m2.result, "merged");
  assert.equal(m2.preSha, base);
  assert.equal(m2.postSha, await head(f.leadWorktree));

  const m1 = await merge(f, j1);
  assert.equal(m1.result, "merged");
  assert.equal(m1.preSha, m2.postSha);
  assert.equal(m1.postSha, await head(f.leadWorktree));

  assert.equal(
    await log(f.leadWorktree, `${base}..${f.leadBranch}`),
    ["Worker One|j1: second commit", "Worker One|j1: first commit", "Worker Two|j2: add j2.txt"].join("\n"),
  );
  assert.equal(await isLinear(f.leadWorktree, f.leadBranch), true);
  const newShas = (await log(f.leadWorktree, `${m2.postSha}..${f.leadBranch}`, "%H")).split("\n");
  assert.equal(newShas.length, 2);
  for (const s of newShas) assert.ok(!oldShas.includes(s), "rebased commits have new SHAs");
  for (const f2 of ["j1.txt", "j1b.txt", "j2.txt"]) assert.equal(existsSync(join(f.leadWorktree, f2)), true);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
});

test("gate_failed: Lead untouched, Attempt keeps its commit", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j3-a1", [{ file: "j3.txt", content: "j3\n", message: "j3: add" }]);
  await write(a.path, "FAIL_GATE", "");
  const pre = await head(f.leadWorktree);
  const own = await head(a.path);
  const m = await merge(f, a);
  assert.equal(m.result, "gate_failed");
  assert.match(m.detail, /FAIL_GATE present/);
  assert.equal(m.preSha, pre);
  await assertLeadUntouched(f, pre);
  assert.equal(await head(a.path), own);
});

test("conflict: rebase aborted in the Attempt, Lead untouched, files reported", async () => {
  const f = await createFixture();
  const j4 = await makeAttempt(f, "j4-a1", [{ file: "app.txt", content: "line1\nj4\nline3\nline4\nline5\n", message: "j4: edit", author: W1 }]);
  const j5 = await makeAttempt(f, "j5-a1", [{ file: "app.txt", content: "line1\nj5\nline3\nline4\nline5\n", message: "j5: edit", author: W2 }]);
  assert.equal((await merge(f, j4)).result, "merged");
  const pre = await head(f.leadWorktree);
  const own = await head(j5.path);
  const m = await merge(f, j5);
  assert.equal(m.result, "conflict");
  assert.deepEqual(m.files, ["app.txt"]);
  await assertLeadUntouched(f, pre);
  assert.equal(await rebaseInProgress(j5.path), false);
  assert.equal(await head(j5.path), own);
  assert.deepEqual(await statusPorcelain(j5.path), []);
});

test("dirty_target: a dirty Lead worktree is refused before anything runs", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j8-a1", [{ file: "j8.txt", content: "j8\n", message: "j8: add" }]);
  await write(f.leadWorktree, "README.md", "dirty\n");
  const pre = await head(f.leadWorktree);
  const m = await merge(f, a);
  assert.equal(m.result, "dirty_target");
  assert.equal(await head(f.leadWorktree), pre);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), [" M README.md"]);
  assert.equal(existsSync(join(f.leadWorktree, "j8.txt")), false);
});

/** A script run by the fixture's verify.sh (env CAS_HOOK) at most once, to act on the Lead mid-Gate. */
function gateHook(body: string): void {
  const dir = tmpDir("cpd-hook-");
  const script = join(dir, "hook.sh");
  writeFileSync(script, `#!/bin/sh\n[ -e '${dir}/ran' ] && exit 0\ntouch '${dir}/ran'\n${body}\n`);
  chmodSync(script, 0o755);
  process.env.CAS_HOOK = script;
}

test("dirty_target: the Lead becomes dirty during the Gate, wt refuses, Lead ref unchanged", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j8-a1", [{ file: "app.txt", content: "line1\nj8\nline3\nline4\nline5\n", message: "j8: edit" }]);
  gateHook(`echo dirt >> '${f.leadWorktree}/app.txt'`);
  try {
    const pre = await head(f.leadWorktree);
    const m = await merge(f, a);
    assert.equal(m.result, "dirty_target");
    assert.equal(await head(f.leadWorktree), pre);
    assert.deepEqual(await statusPorcelain(f.leadWorktree), [" M app.txt"]);
  } finally {
    delete process.env.CAS_HOOK;
  }
});

test("cas_lost: the Lead moves during the Gate; with no retries it is reported, and a rerun succeeds", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j6-a1", [{ file: "j6.txt", content: "j6\n", message: "j6: add" }]);
  gateHook(`cd '${f.leadWorktree}' && echo moved > moved.txt && git add moved.txt && git commit -q -m 'lead: moved'`);
  try {
    const pre = await head(f.leadWorktree);
    const m = await merge(f, a, { casRetries: 0 });
    assert.equal(m.result, "cas_lost");
    assert.equal(m.preSha, pre);
    const moved = await head(f.leadWorktree);
    assert.notEqual(moved, pre);
    assert.equal(await log(f.leadWorktree, "HEAD", "%s"), "lead: moved\ninitial");
    assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
    assert.equal(await rebaseInProgress(a.path), false);

    const again = await merge(f, a, { casRetries: 0 });
    assert.equal(again.result, "merged");
    assert.equal(again.preSha, moved);
  } finally {
    delete process.env.CAS_HOOK;
  }
});

test("cas_lost is retried automatically and the retry succeeds", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j6-a1", [{ file: "j6.txt", content: "j6\n", message: "j6: add" }]);
  gateHook(`cd '${f.leadWorktree}' && echo moved > moved.txt && git add moved.txt && git commit -q -m 'lead: moved'`);
  try {
    const m = await merge(f, a);
    assert.equal(m.result, "merged");
    assert.equal(await log(f.leadWorktree, "HEAD", "%s"), "j6: add\nlead: moved\ninitial");
    assert.equal(m.postSha, await head(f.leadWorktree));
  } finally {
    delete process.env.CAS_HOOK;
  }
});

test("tampered: an Attempt that edits .config/wt.toml is refused, Lead untouched", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j9-a1", [
    { file: ".config/wt.toml", content: `[pre-merge]\nverify = "true"\n`, message: "j9: weaken gate" },
  ]);
  const pre = await head(f.leadWorktree);
  const m = await merge(f, a);
  assert.equal(m.result, "tampered");
  assert.deepEqual(m.files, [".config/wt.toml"]);
  await assertLeadUntouched(f, pre);
});

test("tampered: an Attempt that only renames .config/wt.toml is refused, Lead untouched", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j9-a1");
  await gitOut(a.path, ["mv", ".config/wt.toml", ".config/wt.toml.bak"]);
  await gitOut(a.path, ["commit", "-q", "-m", "j9: rename gate config away"]);
  const pre = await head(f.leadWorktree);
  const m = await merge(f, a);
  assert.equal(m.result, "tampered");
  assert.ok(m.files?.includes(".config/wt.toml"), `files: ${m.files}`);
  assert.equal(m.preSha, pre);
  await assertLeadUntouched(f, pre);
  assert.equal(existsSync(join(f.leadWorktree, ".config/wt.toml.bak")), false);
});

test("an Attempt with uncommitted changes is refused with attempt_dirty", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j9-a1", [{ file: "j9.txt", content: "j9\n", message: "j9: add" }]);
  await write(a.path, "stray.txt", "x");
  const pre = await head(f.leadWorktree);
  await assert.rejects(merge(f, a), (e: any) => e.code === "attempt_dirty" && e.exitCode === 3);
  await assertLeadUntouched(f, pre);
});

test("post_gate_failed: the Lead Gate fails after the merge, the Lead is rolled back", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j9-a1", [{ file: "j9.txt", content: "j9\n", message: "j9: add" }]);
  await write(f.leadWorktree, "FAIL_GATE", ""); // ignored: the Lead stays clean, only its Gate fails
  const pre = await head(f.leadWorktree);
  const m = await merge(f, a);
  assert.equal(m.result, "post_gate_failed");
  assert.equal(m.preSha, pre);
  assert.equal(m.postSha, undefined);
  await assertLeadUntouched(f, pre);
  assert.equal(existsSync(join(f.leadWorktree, "j9.txt")), false);
});

test("concurrent merges are serialised by the merge lock: both land, linear", async () => {
  const f = await createFixture();
  const a = await makeAttempt(f, "j6-a1", [{ file: "j6.txt", content: "j6\n", message: "j6: add" }]);
  const b = await makeAttempt(f, "j7-a1", [{ file: "j7.txt", content: "j7\n", message: "j7: add" }]);
  const [ma, mb] = await Promise.all([merge(f, a), merge(f, b)]);
  assert.equal(ma.result, "merged");
  assert.equal(mb.result, "merged");
  assert.equal(await isLinear(f.leadWorktree, f.leadBranch), true);
  assert.deepEqual(await statusPorcelain(f.leadWorktree), []);
  for (const x of [a, b]) await removeAttemptWorktree(f.repo, f.leadWorktree, x.branch);
});
