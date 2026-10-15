import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { SUBMIT_NEXT, submitAttempt, type SubmitDeps } from "../../src/dispatch/attempt-submit.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { reviewGatePath } from "../../src/lib/paths.ts";
import { withFakeWt } from "../helpers/index.ts";
import { commitFile, createFixture, git, head, makeAttempt, sandbox, type Fixture } from "../git/fixtures.ts";

// Real git and wt in temp repos; only the Ledger is faked.
sandbox();

const WF = "demo";
let f: Fixture;
let n = 0;

test("setup: a repo with a Lead Branch worktree", async () => {
  f = await createFixture();
});

type Setup = { id: string; path: string; deps: SubmitDeps; submitted: string[] };

/** A new Attempt (own worktree) with `commits`, and the faked Ledger side. */
async function attempt(commits: { file: string; content: string; message: string }[] = [], status = "in_progress"): Promise<Setup> {
  const id = `api-a${++n}`;
  const { path } = await makeAttempt(f, id, commits);
  const submitted: string[] = [];
  return {
    id,
    path,
    submitted,
    deps: {
      wf: WF,
      attempt: { id, status, worktree: path },
      leadBranch: f.leadBranch,
      reviewMode: "fork",
      markSubmitted: async (sha) => void submitted.push(sha),
    },
  };
}

const work = [{ file: "src/a.txt", content: "a\n", message: "api: add a" }];

async function refusal(p: Promise<unknown>, code: string): Promise<CpdError> {
  try {
    await p;
  } catch (e) {
    assert.ok(e instanceof CpdError, String(e));
    assert.equal(e.code, code, `${e.code}: ${e.message}`);
    assert.equal(e.exitCode, 3, "a refusal is exit 3");
    return e;
  }
  assert.fail(`expected refusal ${code}`);
}

test("wrong_worktree: cwd outside the Attempt worktree (primary checkout, another Attempt's worktree)", async () => {
  const a = await attempt(work);
  const other = await attempt(work);
  await refusal(submitAttempt(f.repo, a.deps), "wrong_worktree");
  await refusal(submitAttempt(other.path, a.deps), "wrong_worktree");
  await refusal(submitAttempt(join(a.path, ".."), a.deps), "wrong_worktree");
  assert.deepEqual(a.submitted, []);
});

test("wrong_worktree: no worktree recorded for the Attempt", async () => {
  const a = await attempt(work);
  await refusal(submitAttempt(a.path, { ...a.deps, attempt: { ...a.deps.attempt, worktree: null } }), "wrong_worktree");
});

test("wrong_worktree: the worktree is on another branch than the Attempt branch (the Gate would not see what is merged)", async () => {
  const a = await attempt(work);
  await git(a.path, ["switch", "-q", "-c", "elsewhere"]);
  const e = await refusal(submitAttempt(a.path, a.deps), "wrong_worktree");
  assert.equal(e.details.actual, "elsewhere");
  await git(a.path, ["switch", "-q", `cpd/demo--${a.id}`]);
  assert.equal((await submitAttempt(a.path, a.deps)).next, SUBMIT_NEXT);
});

test("the Attempt must be running: submitting a closed or reviewed Attempt is illegal_transition", async () => {
  const a = await attempt(work, "in_review");
  await refusal(submitAttempt(a.path, a.deps), "illegal_transition");
});

test("dirty_tree: uncommitted change and untracked file", async () => {
  const a = await attempt(work);
  writeFileSync(join(a.path, "README.md"), "changed\n");
  const e = await refusal(submitAttempt(a.path, a.deps), "dirty_tree");
  assert.deepEqual(e.details.status, [" M README.md"]);
  await git(a.path, ["checkout", "--", "README.md"]);
  writeFileSync(join(a.path, "stray.txt"), "x\n");
  await refusal(submitAttempt(a.path, a.deps), "dirty_tree");
  assert.deepEqual(a.submitted, []);
});

test("no_commits: a clean Attempt with nothing on top of the Lead Branch", async () => {
  const a = await attempt([]);
  await refusal(submitAttempt(a.path, a.deps), "no_commits");
});

test("gate_tampered: the diff touches .config/wt.toml, verify.sh or .cpd/ (also by rename)", async () => {
  const cases: [string, (p: string) => Promise<void>][] = [
    ["verify.sh", async (p) => void (await commitFile(p, "verify.sh", "#!/bin/sh\nexit 0\n", "weaken the gate"))],
    [".config/wt.toml", async (p) => void (await commitFile(p, ".config/wt.toml", "[pre-merge]\nverify = \"true\"\n", "weaken the hook"))],
    [".cpd/notes.md", async (p) => void (await commitFile(p, ".cpd/notes.md", "x\n", "touch .cpd"))],
    [
      ".config/wt.toml",
      async (p) => {
        await git(p, ["mv", ".config/wt.toml", ".config/wt.toml.bak"]);
        await git(p, ["commit", "-q", "-m", "rename the hook config"]);
      },
    ],
  ];
  for (const [file, touch] of cases) {
    const a = await attempt(work);
    await touch(a.path);
    const e = await refusal(submitAttempt(a.path, a.deps), "gate_tampered");
    assert.ok((e.details.files as string[]).includes(file), `${file} in ${JSON.stringify(e.details.files)}`);
    assert.deepEqual(a.submitted, []);
  }
});

test("check_failed: exit 64 from the quick check (hook pre-commit), with the output tail in details", async () => {
  const a = await attempt(work);
  const { result, calls } = await withFakeWt(64, "lint: bad", () => submitAttempt(a.path, a.deps));
  const e = (result as PromiseRejectedResult).reason as CpdError;
  assert.equal(result.status, "rejected");
  assert.deepEqual([e.code, e.exitCode, e.details.exitCode, e.details.outputTail], ["check_failed", 3, 64, "lint: bad"]);
  assert.deepEqual(calls, [["--yes", "-C", a.path, "hook", "pre-commit"]]);
  assert.deepEqual(a.submitted, []);
  assert.equal(existsSync(reviewGatePath(WF, a.id)), false, "no output file for a failed check");
});

test("check_error: any other failure of the quick check", async () => {
  const a = await attempt(work);
  const { result } = await withFakeWt(42, "wt broke", () => submitAttempt(a.path, a.deps));
  const e = (result as PromiseRejectedResult).reason as CpdError;
  assert.deepEqual([e.code, e.details.code], ["check_error", 42]);
  assert.deepEqual(a.submitted, []);
});

test("no pre-commit table means no commands: it passes, and does not fall back to the failing pre-merge Gate", async () => {
  const a = await attempt(work);
  writeFileSync(join(a.path, "FAIL_GATE"), ""); // the fixture has a pre-merge table and no pre-commit table; FAIL_GATE fails pre-merge only; ignored, so the tree stays clean
  assert.equal((await submitAttempt(a.path, a.deps)).next, SUBMIT_NEXT);
});

test("preconditions run in order: dirty_tree, no_commits, gate_tampered", async () => {
  const a = await attempt([]);
  writeFileSync(join(a.path, "FAIL_GATE"), "");
  writeFileSync(join(a.path, "dirty.txt"), "x");
  await refusal(submitAttempt(a.path, a.deps), "dirty_tree"); // before no_commits
  writeFileSync(join(a.path, "dirty.txt"), "");
  await git(a.path, ["clean", "-fdq"]);
  await refusal(submitAttempt(a.path, a.deps), "no_commits"); // before the quick check
  await commitFile(a.path, "verify.sh", "#!/bin/sh\nexit 1\n", "tamper and fail");
  await refusal(submitAttempt(a.path, a.deps), "gate_tampered"); // before the quick check
});

test("success: the quick-check output is written for the Reviewer, then the HEAD commit is recorded; a subdirectory is fine", async () => {
  const a = await attempt(work);
  mkdirSync(join(a.path, "src"), { recursive: true });
  const r = await submitAttempt(join(a.path, "src"), a.deps);
  assert.equal(r.next, "Stop now. Do not make more changes.");
  assert.equal(r.commit, await head(a.path));
  assert.deepEqual(a.submitted, [await head(a.path)]);
  assert.ok(existsSync(reviewGatePath(WF, a.id)));
  assert.match(readFileSync(reviewGatePath(WF, a.id), "utf8"), /pre-commit/i);
});

test("the quick-check output file is written before the Ledger records the submission", async () => {
  const a = await attempt(work);
  let existedAtRecord = false;
  await submitAttempt(a.path, {
    ...a.deps,
    markSubmitted: async () => {
      existedAtRecord = existsSync(reviewGatePath(WF, a.id));
    },
  });
  assert.equal(existedAtRecord, true);
});

test("a Ledger refusal on markSubmitted propagates (nothing is swallowed)", async () => {
  const a = await attempt(work);
  await assert.rejects(
    submitAttempt(a.path, {
      ...a.deps,
      markSubmitted: async () => {
        throw new CpdError("illegal_transition", "already submitted", 3);
      },
    }),
    (e: unknown) => e instanceof CpdError && e.code === "illegal_transition",
  );
});

test("check_failed details carry only the last 60 lines of a long output", async () => {
  const a = await attempt(work);
  const { result } = await withFakeWt(64, Array.from({ length: 200 }, (_, i) => `check line ${i + 1}`).join("\n"), () => submitAttempt(a.path, a.deps));
  const e = (result as PromiseRejectedResult).reason as CpdError;
  assert.equal(e.code, "check_failed");
  const lines = String(e.details.outputTail).split("\n");
  assert.ok(lines.length <= 60, `${lines.length} lines`);
  assert.ok(lines.includes("check line 200"), "the end of the output is kept");
  assert.ok(!lines.includes("check line 1"));
});

test("reviewMode none: no review file is written, the submission is still recorded", async () => {
  const a = await attempt(work);
  await submitAttempt(a.path, { ...a.deps, reviewMode: "none" });
  assert.equal(existsSync(reviewGatePath(WF, a.id)), false);
  assert.deepEqual(a.submitted, [await head(a.path)]);
});
