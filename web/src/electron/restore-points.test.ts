import { expect, it } from "vitest";
import { createRestorePoints } from "./restore-points.ts";

const WT = "/wt";
const DIST = "/wt/web/dist-electron";
const CAND = "/wt/web/dist-electron.candidate";
const LAST = "/wt/web/dist-electron.last-good";
const STAGE = "/wt/web/dist-electron.restore";
const REF = "refs/cpd/preview-last-good";
const GIT_DIR = "/wt/.git";

interface GitCall {
  file: string;
  args: string[];
  cwd: string;
  env: Record<string, string>;
}

function setup(
  opts: {
    refTree?: string | null;
    writeTrees?: string[];
    existing?: string[];
    added?: string[];
  } = {},
) {
  const ops: string[] = []; // ordered: "git <args>" and "fs:<op> ..."
  const gitCalls: GitCall[] = [];
  const trees = [...(opts.writeTrees ?? ["treeA"])];
  const existing = new Set(opts.existing ?? []);
  const refTree = opts.refTree === undefined ? null : opts.refTree;
  const exec = async (
    file: string,
    args: string[],
    o: { cwd: string; env: Record<string, string> },
  ) => {
    gitCalls.push({ file, args, cwd: o.cwd, env: o.env });
    ops.push(`${file} ${args.join(" ")}`);
    if (args[0] === "rev-parse" && args[1] === "--absolute-git-dir") return { stdout: `${GIT_DIR}\n` };
    if (args[0] === "rev-parse" && args[1] === "--verify") {
      if (refTree === null) throw new Error("exit 1");
      return { stdout: `${refTree}\n` };
    }
    if (args[0] === "write-tree") return { stdout: `${trees.length > 1 ? trees.shift() : trees[0]}\n` };
    if (args[0] === "commit-tree") return { stdout: "commit1\n" };
    if (args[0] === "diff-tree") return { stdout: (opts.added ?? []).map((f) => `${f}\0`).join("") };
    return { stdout: "" };
  };
  const fs = {
    cp: async (a: string, b: string) => {
      ops.push(`fs:cp ${a} ${b}`);
    },
    rm: async (p: string) => {
      ops.push(`fs:rm ${p}`);
    },
    rename: async (a: string, b: string) => {
      ops.push(`fs:rename ${a} ${b}`);
    },
    exists: async (p: string) => existing.has(p),
  };
  const rp = createRestorePoints({ worktree: WT, exec, fs });
  const git = () => gitCalls.filter((c) => !(c.args[0] === "rev-parse" && c.args[1] === "--absolute-git-dir"));
  const gitArgs = () => git().map((c) => c.args);
  return { rp, ops, gitCalls, git, gitArgs };
}

function subsequence(ops: string[], expected: string[]) {
  let i = 0;
  for (const op of ops) if (i < expected.length && op === expected[i]) i++;
  return i === expected.length;
}

function checkEnv(calls: GitCall[]) {
  for (const c of calls) {
    expect(c.cwd).toBe(WT);
    if (c.args[0] === "rev-parse" && c.args[1] === "--absolute-git-dir") continue;
    expect(c.env.GIT_INDEX_FILE).toMatch(/cpd-preview-index$/);
    expect(c.env.GIT_INDEX_FILE).toBe(`${GIT_DIR}/cpd-preview-index`);
  }
}

const forbidden = ["checkout", "reset", "clean", "stash"];

it("capture builds a tree through a temporary index and never touches the real index", async () => {
  const s = setup();
  await s.rp.captureCandidate();
  expect(s.gitCalls[0]?.args).toEqual(["rev-parse", "--absolute-git-dir"]);
  expect(s.gitArgs()).toEqual([["read-tree", "HEAD"], ["add", "-A"], ["write-tree"]]);
  checkEnv(s.gitCalls);
  for (const c of s.gitCalls) for (const f of forbidden) expect(c.args).not.toContain(f);
});

it("capture copies dist-electron to the candidate directory", async () => {
  const s = setup({ existing: [CAND] });
  await s.rp.captureCandidate();
  expect(s.ops).toContain(`fs:cp ${DIST} ${CAND}`);
  // an older candidate is replaced: removed before the copy
  const rm = s.ops.indexOf(`fs:rm ${CAND}`);
  const cp = s.ops.indexOf(`fs:cp ${DIST} ${CAND}`);
  expect(rm).toBeGreaterThanOrEqual(0);
  expect(rm).toBeLessThan(cp);
});

it("promote commits the tree on refs/cpd/preview-last-good and swaps the candidate in", async () => {
  const s = setup({ refTree: "oldTree", writeTrees: ["newTree"] });
  await s.rp.captureCandidate();
  await s.rp.promoteCandidate();
  expect(s.gitArgs()).toEqual([
    ["read-tree", "HEAD"],
    ["add", "-A"],
    ["write-tree"],
    ["rev-parse", "--verify", "-q", `${REF}^{tree}`],
    ["commit-tree", "newTree", "-p", "HEAD", "-m", "cpd preview last good"],
    ["update-ref", REF, "commit1"],
  ]);
  checkEnv(s.gitCalls);
  const commit = s.git().find((c) => c.args[0] === "commit-tree");
  expect(commit?.env.GIT_AUTHOR_NAME).toBe("cpd-preview-watchdog");
  expect(commit?.env.GIT_COMMITTER_NAME).toBe("cpd-preview-watchdog");
  expect(commit?.env.GIT_AUTHOR_EMAIL).toBe("cpd-preview-watchdog@localhost");
  expect(commit?.env.GIT_COMMITTER_EMAIL).toBe("cpd-preview-watchdog@localhost");
  expect(subsequence(s.ops, [`fs:rm ${LAST}`, `fs:rename ${CAND} ${LAST}`])).toBe(true);
  const updateRef = s.ops.findIndex((o) => o.startsWith("git update-ref"));
  expect(updateRef).toBeLessThan(s.ops.indexOf(`fs:rename ${CAND} ${LAST}`));
});

it("promote skips the commit when the tree equals the last good tree", async () => {
  const s = setup({ refTree: "same", writeTrees: ["same"] });
  await s.rp.captureCandidate();
  await s.rp.promoteCandidate();
  const names = s.gitArgs().map((a) => a[0]);
  expect(names).not.toContain("commit-tree");
  expect(names).not.toContain("update-ref");
  expect(subsequence(s.ops, [`fs:rm ${LAST}`, `fs:rename ${CAND} ${LAST}`])).toBe(true);
});

it("restore replaces dist-electron from last-good through a staging directory", async () => {
  const s = setup({ refTree: "good", writeTrees: ["cur"] });
  await s.rp.restoreLastGood();
  expect(
    subsequence(s.ops, [`fs:cp ${LAST} ${STAGE}`, `fs:rm ${DIST}`, `fs:rename ${STAGE} ${DIST}`]),
  ).toBe(true);
  const firstGit = s.ops.findIndex((o) => o.startsWith("git "));
  expect(s.ops.indexOf(`fs:rename ${STAGE} ${DIST}`)).toBeLessThan(
    s.ops.findIndex((o) => o.startsWith("git read-tree HEAD")),
  );
  expect(firstGit).toBeGreaterThanOrEqual(0);
});

it("restore deletes files added since the last good tree and checks out the rest", async () => {
  const s = setup({ refTree: "good", writeTrees: ["cur"], added: ["web/src/new.ts", "docs/x.md"] });
  await s.rp.restoreLastGood();
  expect(
    subsequence(s.ops, [
      "git read-tree HEAD",
      "git add -A",
      "git write-tree",
      "git diff-tree -r -z --name-only --diff-filter=A good cur",
      "fs:rm /wt/web/src/new.ts",
      "fs:rm /wt/docs/x.md",
      `git read-tree ${REF}`,
      "git checkout-index -a -f",
    ]),
  ).toBe(true);
  checkEnv(s.gitCalls);
});

it("restore never runs checkout, reset or clean", async () => {
  const s = setup({ refTree: "good", writeTrees: ["cur"], added: ["a.txt"] });
  await s.rp.restoreLastGood();
  expect(s.gitCalls.length).toBeGreaterThan(0);
  for (const c of s.gitCalls) {
    for (const f of forbidden) expect(c.args).not.toContain(f);
  }
  checkEnv(s.gitCalls);
});

it("hasLastGood is true only when the ref and the directory exist", async () => {
  const both = setup({ refTree: "t", existing: [LAST] });
  expect(await both.rp.hasLastGood()).toBe(true);
  const noDir = setup({ refTree: "t", existing: [] });
  expect(await noDir.rp.hasLastGood()).toBe(false);
  const noRef = setup({ refTree: null, existing: [LAST] });
  expect(await noRef.rp.hasLastGood()).toBe(false);
});
