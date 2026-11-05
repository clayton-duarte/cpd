export interface RestoreExec {
  (
    file: string,
    args: string[],
    opts: { cwd: string; env: Record<string, string> },
  ): Promise<{ stdout: string }>;
}

export interface RestoreFs {
  cp(src: string, dst: string): Promise<void>;
  rm(path: string): Promise<void>;
  rename(a: string, b: string): Promise<void>;
  exists(p: string): Promise<boolean>;
}

export interface RestorePointsDeps {
  worktree: string;
  exec: RestoreExec;
  fs: RestoreFs;
  ref?: "refs/cpd/preview-last-good";
}

export interface RestorePoints {
  captureCandidate(): Promise<void>;
  promoteCandidate(): Promise<void>;
  hasLastGood(): Promise<boolean>;
  restoreLastGood(): Promise<void>;
  discardCandidate(): Promise<void>;
}

const IDENTITY = {
  GIT_AUTHOR_NAME: "cpd-preview-watchdog",
  GIT_COMMITTER_NAME: "cpd-preview-watchdog",
  GIT_AUTHOR_EMAIL: "cpd-preview-watchdog@localhost",
  GIT_COMMITTER_EMAIL: "cpd-preview-watchdog@localhost",
};

export function createRestorePoints(deps: RestorePointsDeps): RestorePoints {
  const { worktree, exec, fs } = deps;
  const ref = deps.ref ?? "refs/cpd/preview-last-good";
  const web = `${worktree}/web`;
  const dist = `${web}/dist-electron`;
  const candidateDir = `${web}/dist-electron.candidate`;
  const lastGoodDir = `${web}/dist-electron.last-good`;
  const staging = `${web}/dist-electron.restore`;

  let indexFile: string | null = null;
  let candidateTree: string | null = null;

  async function git(args: string[], extraEnv: Record<string, string> = {}): Promise<string> {
    if (indexFile === null) {
      const out = await exec("git", ["rev-parse", "--absolute-git-dir"], { cwd: worktree, env: {} });
      indexFile = `${out.stdout.trim()}/cpd-preview-index`;
    }
    const out = await exec("git", args, {
      cwd: worktree,
      env: { GIT_INDEX_FILE: indexFile, ...extraEnv },
    });
    return out.stdout.trim();
  }

  async function currentTree(): Promise<string> {
    await git(["read-tree", "HEAD"]);
    await git(["add", "-A"]);
    return git(["write-tree"]);
  }

  async function refTree(): Promise<string | null> {
    try {
      const out = await git(["rev-parse", "--verify", "-q", `${ref}^{tree}`]);
      return out === "" ? null : out;
    } catch {
      return null;
    }
  }

  return {
    async captureCandidate() {
      candidateTree = await currentTree();
      if (await fs.exists(candidateDir)) await fs.rm(candidateDir);
      await fs.cp(dist, candidateDir);
    },

    async promoteCandidate() {
      if (candidateTree === null) throw new Error("no candidate captured");
      const tree = candidateTree;
      if (tree !== (await refTree())) {
        const commit = await git(["commit-tree", tree, "-p", "HEAD", "-m", "cpd preview last good"], IDENTITY);
        await git(["update-ref", ref, commit]);
      }
      await fs.rm(lastGoodDir);
      await fs.rename(candidateDir, lastGoodDir);
    },

    async hasLastGood() {
      return (await refTree()) !== null && (await fs.exists(lastGoodDir));
    },

    async restoreLastGood() {
      const good = await refTree();
      if (good === null) throw new Error("no last good ref");
      await fs.rm(staging);
      await fs.cp(lastGoodDir, staging);
      await fs.rm(dist);
      await fs.rename(staging, dist);
      const current = await currentTree();
      const added = await git(["diff-tree", "-r", "-z", "--name-only", "--diff-filter=A", good, current]);
      // git() trims; NUL separators survive, only outer whitespace is dropped.
      for (const rel of added.split("\0")) {
        if (rel !== "") await fs.rm(`${worktree}/${rel}`);
      }
      await git(["read-tree", ref]);
      await git(["checkout-index", "-a", "-f"]);
    },

    async discardCandidate() {
      candidateTree = null;
      await fs.rm(candidateDir);
    },
  };
}
