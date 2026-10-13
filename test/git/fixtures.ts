import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { git, gitOut, realpath } from "../../src/git/git.ts";
import { createAttemptWorktree } from "../../src/git/attempt-worktree.ts";
import { createLeadBranch } from "../../src/git/lead-branch.ts";
import { tmpDir } from "../helpers/index.ts";

/** Sandbox HOME/XDG/CPD_HOME for this test process (node --test runs each file in its own process). */
export function sandbox(): void {
  const root = realpath(tmpDir("cpd-git-home-"));
  mkdirSync(join(root, "home", ".config"), { recursive: true });
  process.env.HOME = join(root, "home");
  process.env.XDG_CONFIG_HOME = join(root, "home", ".config");
  process.env.CPD_HOME = join(root, "cpd");
  process.env.GIT_CONFIG_GLOBAL = "/dev/null";
  process.env.GIT_CONFIG_NOSYSTEM = "1";
  delete process.env.CAS_HOOK;
}

export type Author = { name: string; email: string };
export const LEAD_AUTHOR: Author = { name: "Lead Person", email: "lead@example.com" };
export const W1: Author = { name: "Worker One", email: "w1@example.com" };
export const W2: Author = { name: "Worker Two", email: "w2@example.com" };

const SETUP = `sleep 0.7 && node -e 'require("fs").writeFileSync(".setup-done", String(Date.now()))'`;

const WT_TOML = (preStart: string) => `[pre-start]
setup = '''${preStart}'''

[pre-merge]
verify = "./verify.sh || exit 64"
`;

/** Fails when FAIL_GATE exists; CAS_HOOK (an env var naming a script) lets a test move the Lead mid-Gate. */
const VERIFY = `#!/bin/sh
if [ -n "\${CAS_HOOK:-}" ]; then "$CAS_HOOK"; fi
if [ -e FAIL_GATE ]; then echo "verify: FAIL_GATE present" >&2; exit 1; fi
echo "verify: ok"
`;

export async function write(dir: string, file: string, content: string): Promise<void> {
  const p = join(dir, file);
  mkdirSync(join(p, ".."), { recursive: true });
  writeFileSync(p, content);
}

/** Write `file`, commit it as `author`. Returns the new commit SHA. */
export async function commitFile(
  dir: string,
  file: string,
  content: string,
  message: string,
  author: Author = LEAD_AUTHOR,
): Promise<string> {
  await write(dir, file, content);
  await gitOut(dir, ["add", "--", file]);
  await gitOut(dir, ["commit", "-q", "-m", message, `--author=${author.name} <${author.email}>`]);
  return gitOut(dir, ["rev-parse", "HEAD"]);
}

export type Fixture = { repo: string; leadBranch: string; leadWorktree: string };

/** A git repo on `main` with the CPD Gate wiring (`.config/wt.toml`, `verify.sh`). */
export async function createRepo(opts: { preStart?: string } = {}): Promise<string> {
  const repo = join(realpath(tmpDir("cpd-git-fx-")), "repo");
  mkdirSync(repo);
  await gitOut(repo, ["init", "-q", "-b", "main"]);
  await gitOut(repo, ["config", "user.name", LEAD_AUTHOR.name]);
  await gitOut(repo, ["config", "user.email", LEAD_AUTHOR.email]);
  await write(repo, ".config/wt.toml", WT_TOML(opts.preStart ?? SETUP));
  await write(repo, "verify.sh", VERIFY);
  chmodSync(join(repo, "verify.sh"), 0o755);
  await write(repo, ".gitignore", ".setup-done\nFAIL_GATE\n");
  await write(repo, "app.txt", "line1\nline2\nline3\nline4\nline5\n");
  await write(repo, "README.md", "# fixture\n");
  await gitOut(repo, ["add", "-A"]);
  await gitOut(repo, ["commit", "-q", "-m", "initial", `--author=${LEAD_AUTHOR.name} <${LEAD_AUTHOR.email}>`]);
  return repo;
}

/** Repo plus a Lead Branch in its own worktree. */
export async function createFixture(opts: { preStart?: string } = {}): Promise<Fixture> {
  const repo = await createRepo(opts);
  const leadBranch = "cpd/demo";
  const { path } = await createLeadBranch(repo, leadBranch, "main");
  return { repo, leadBranch, leadWorktree: path };
}

export const attemptBranch = (f: Fixture, id: string) => `${f.leadBranch}--${id}`;

/** Create an Attempt worktree; optionally commit files in it as `author`. */
export async function makeAttempt(
  f: Fixture,
  id: string,
  commits: { file: string; content: string; message: string; author?: Author }[] = [],
): Promise<{ path: string; branch: string }> {
  const branch = attemptBranch(f, id);
  const { path } = await createAttemptWorktree(f.leadWorktree, branch, f.leadBranch);
  for (const c of commits) await commitFile(path, c.file, c.content, c.message, c.author ?? W1);
  return { path, branch };
}

export const head = (dir: string, ref = "HEAD") => gitOut(dir, ["rev-parse", ref]);
export const log = (dir: string, ref: string, fmt = "%an|%s") => gitOut(dir, ["log", "--format=" + fmt, ref]);
export const isLinear = async (dir: string, ref: string) =>
  (await gitOut(dir, ["rev-list", "--merges", ref])) === "";
export { git };
