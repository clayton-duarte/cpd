import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { primaryCheckoutMove, reasonText } from "../lib/mainguard.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.16: no agent moves HEAD or changes the files of a repo's primary checkout.

const SEVEN = ["checkout", "switch", "reset", "rebase", "merge", "pull", "stash"] as const;
const ARGS: Record<string, string> = {
  checkout: "checkout -q --detach origin/main",
  switch: "switch -c x",
  reset: "reset --hard origin/main",
  rebase: "rebase origin/main",
  merge: "merge origin/main",
  pull: "pull",
  stash: "stash",
};
const PREFIX = "φ: no agent moves HEAD or changes the files of a repo's primary checkout";
const ALTERNATIVES = ["git show origin/main:<path>", "git log origin/main", "git diff origin/main", "git worktree add $(mktemp -d) origin/main"];

const cleanEnv = () => Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith("GIT_"))) as NodeJS.ProcessEnv;
const git = (cwd: string, ...args: string[]) => execFileSync("git", args, { cwd, env: cleanEnv(), stdio: "pipe" });

type Fx = { tmp: string; primary: string; wt: string; inside: string; plain: string; missing: string };

function fixtures(t: TestContext): Fx {
  const tmp = mkdtempSync(join(tmpdir(), "mainguard-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "cpd");
  mkdirSync(primary, { recursive: true });
  git(primary, "init", "-q", "-b", "main");
  git(primary, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x");
  mkdirSync(join(primary, "src"));
  const wt = join(tmp, "projects", "cpd.wt");
  git(primary, "worktree", "add", "-q", "-b", "wt", wt);
  mkdirSync(join(wt, "src"));
  const inside = join(primary, "inside");
  git(primary, "worktree", "add", "-q", "-b", "inner", inside);
  mkdirSync(join(inside, "src"));
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  return { tmp, primary, wt, inside, plain, missing: join(tmp, "does-not-exist") };
}

test("refuses checkout, switch, reset, rebase, merge, pull and stash after a cd into a primary checkout", (t) => {
  const x = fixtures(t);
  for (const s of SEVEN) {
    const want = `git ${s} in ${x.primary}`;
    assert.equal(primaryCheckoutMove(`cd ${x.primary} && git ${ARGS[s]}`, x.plain, x.tmp), want, s);
    assert.equal(primaryCheckoutMove(`cd ${x.primary}; git ${ARGS[s]}`, x.plain, x.tmp), want, s);
    assert.equal(primaryCheckoutMove(`cd ${x.primary} && true | git ${ARGS[s]}`, x.plain, x.tmp), want, s);
  }
  assert.equal(primaryCheckoutMove(`cd ${x.primary} && git stash list`, x.plain, x.tmp), `git stash in ${x.primary}`);
  assert.equal(primaryCheckoutMove(`cd ${x.primary} && git checkout -- file`, x.plain, x.tmp), `git checkout in ${x.primary}`);
});

test("refuses the same seven with git -C, chained -C and from a subdirectory of the primary checkout", (t) => {
  const x = fixtures(t);
  for (const s of SEVEN) {
    const want = `git ${s} in ${x.primary}`;
    assert.equal(primaryCheckoutMove(`git -C ${x.primary} ${ARGS[s]}`, x.plain, x.tmp), want, s);
    assert.equal(primaryCheckoutMove(`git -C ${join(x.tmp, "projects")} -C cpd ${ARGS[s]}`, x.plain, x.tmp), want, s);
    assert.equal(primaryCheckoutMove(`cd ${x.primary}/src && git ${ARGS[s]}`, x.plain, x.tmp), want, s);
    assert.equal(primaryCheckoutMove(`git -C .. ${ARGS[s]}`, join(x.primary, "src"), x.tmp), want, s);
  }
});

test("refuses the incident command with a tilde and the home directory resolved", (t) => {
  const x = fixtures(t);
  for (const [c, s] of [
    ["cd ~/projects/cpd && git checkout -q --detach origin/main", "checkout"],
    ["cd $HOME/projects/cpd && git reset --hard", "reset"],
    ["cd ${HOME}/projects/cpd; git pull", "pull"],
  ]) assert.equal(primaryCheckoutMove(c, x.plain, x.tmp), `git ${s} in ${x.primary}`, c);
});

test("refuses a bare git command when the session cwd is in a primary checkout", (t) => {
  const x = fixtures(t);
  assert.equal(primaryCheckoutMove("git checkout main", x.primary, x.tmp), `git checkout in ${x.primary}`);
  assert.equal(primaryCheckoutMove("git checkout main", join(x.primary, "src"), x.tmp), `git checkout in ${x.primary}`);
  assert.equal(primaryCheckoutMove("git checkout main", x.wt, x.tmp), undefined);
});

const READS = ["fetch origin", "fetch", "log origin/main", "show origin/main:README.md", "diff origin/main", "status", "grep -n x", "worktree add /tmp/somewhere origin/main", "worktree remove /tmp/somewhere", "branch", "rev-parse HEAD"];

test("passes fetch, log, show, diff, status, grep, worktree add and worktree remove in a primary checkout", (t) => {
  const x = fixtures(t);
  for (const r of READS) {
    assert.equal(primaryCheckoutMove(`cd ${x.primary} && git ${r}`, x.plain, x.tmp), undefined, r);
    assert.equal(primaryCheckoutMove(`git -C ${x.primary} ${r}`, x.plain, x.tmp), undefined, r);
    assert.equal(primaryCheckoutMove(`git ${r}`, x.primary, x.tmp), undefined, r);
  }
});

test("passes the four alternatives the refusal names", (t) => {
  const x = fixtures(t);
  for (const r of ["worktree add $(mktemp -d) origin/main", "show origin/main:src/a.ts", "log origin/main", "diff origin/main"]) {
    assert.equal(primaryCheckoutMove(`git -C ${x.primary} ${r}`, x.plain, x.tmp), undefined, r);
    assert.equal(primaryCheckoutMove(`cd ${x.primary} && git ${r}`, x.plain, x.tmp), undefined, r);
  }
});

test("passes every command in a linked worktree, with cd, with -C and from its subdirectories", (t) => {
  const x = fixtures(t);
  for (const dir of [x.wt, join(x.wt, "src"), x.inside, join(x.inside, "src")]) {
    for (const s of SEVEN) {
      assert.equal(primaryCheckoutMove(`cd ${dir} && git ${ARGS[s]}`, x.plain, x.tmp), undefined, `${dir} ${s}`);
      assert.equal(primaryCheckoutMove(`git -C ${dir} ${ARGS[s]}`, x.plain, x.tmp), undefined, `${dir} ${s}`);
      assert.equal(primaryCheckoutMove(`git ${ARGS[s]}`, dir, x.tmp), undefined, `${dir} ${s}`);
    }
  }
});

test("passes outside a git repository and in a directory that does not exist", (t) => {
  const x = fixtures(t);
  for (const dir of [x.plain, x.missing])
    for (const s of SEVEN) {
      assert.equal(primaryCheckoutMove(`cd ${dir} && git ${ARGS[s]}`, x.plain, x.tmp), undefined, `${dir} ${s}`);
      assert.equal(primaryCheckoutMove(`git -C ${dir} ${ARGS[s]}`, x.plain, x.tmp), undefined, `${dir} ${s}`);
      assert.equal(primaryCheckoutMove(`git ${ARGS[s]}`, dir, x.tmp), undefined, `${dir} ${s}`);
    }
});

test("does not judge a directory it cannot resolve, then judges again after a cd that resolves", (t) => {
  const x = fixtures(t);
  for (const c of ["cd $(mktemp -d) && git checkout x", 'cd "$d" && git checkout x', 'git -C "$d" checkout x', "cd - && git checkout x", "cd ~someone && git checkout x"])
    assert.equal(primaryCheckoutMove(c, x.primary, x.tmp), undefined, c);
  assert.equal(primaryCheckoutMove(`cd $(mktemp -d); cd ${x.primary} && git checkout x`, x.plain, x.tmp), `git checkout in ${x.primary}`);
});

test("merge-base and merge-tree are not merge", (t) => {
  const x = fixtures(t);
  assert.equal(primaryCheckoutMove("git merge-base HEAD origin/main", x.primary, x.tmp), undefined);
  assert.equal(primaryCheckoutMove("git merge-tree --write-tree a b", x.primary, x.tmp), undefined);
});

test("git global options before the subcommand are skipped", (t) => {
  const x = fixtures(t);
  assert.equal(primaryCheckoutMove("git -c core.pager=cat --no-pager checkout x", x.primary, x.tmp), `git checkout in ${x.primary}`);
  assert.equal(primaryCheckoutMove("git --git-dir=.git checkout x", x.primary, x.tmp), `git checkout in ${x.primary}`);
  assert.equal(primaryCheckoutMove("git -p reset", x.primary, x.tmp), `git reset in ${x.primary}`);
  assert.equal(primaryCheckoutMove("git --no-pager log", x.primary, x.tmp), undefined);
});

test("prefixes and a path to git are looked through", (t) => {
  const x = fixtures(t);
  for (const [c, s] of [
    ["FOO=1 git checkout x", "checkout"],
    ["command git reset --hard", "reset"],
    ["sudo git pull", "pull"],
    ["time git rebase x", "rebase"],
    ["/usr/bin/git switch x", "switch"],
    ["env FOO=1 git merge x", "merge"],
  ]) assert.equal(primaryCheckoutMove(c, x.primary, x.tmp), `git ${s} in ${x.primary}`, c);
});

test("quoted or echoed text is not a git command", (t) => {
  const x = fixtures(t);
  for (const c of ["echo git checkout x", 'grep "git reset" file', "printf '%s' 'git pull'"])
    assert.equal(primaryCheckoutMove(c, x.primary, x.tmp), undefined, c);
});

test("the refusal text is one line that names the rule and the four alternatives", () => {
  const r = reasonText("git checkout in /p/cpd");
  assert.ok(!r.includes("\n"));
  assert.ok(r.startsWith(PREFIX), r);
  for (const a of ALTERNATIVES) assert.ok(r.includes(a), a);
  assert.ok(r.endsWith("refused: git checkout in /p/cpd"), r);
});

// ---- through the adapter ----

async function started(role: string) {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return f;
}

type Result = { block?: boolean; reason?: string } | undefined;
const call = async (f: Fixture, command: string, toolName = "bash") => (await f.pi.fire("agent_end", { messages: [] }), await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input: { command } }))[0] as Result;
const inCwd = (f: Fixture, dir: string) => {
  const ctx = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => ({ ...ctx(), cwd: dir });
};

const ROLES = ["lead", "manager", "worker"];

test("the adapter blocks a refused command for a Lead, a Manager and a Worker", async (t) => {
  const x = fixtures(t);
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    const r = await call(f, `cd ${x.primary} && git checkout -q --detach origin/main`);
    assert.equal(r?.block, true, role);
    const reason = r?.reason ?? "";
    assert.ok(!reason.includes("\n"), role);
    assert.ok(reason.startsWith(PREFIX), reason);
    for (const a of ALTERNATIVES) assert.ok(reason.includes(a), a);
    assert.ok(reason.endsWith(`refused: git checkout in ${x.primary}`), reason);
  }
});

test("the adapter passes the allowed forms and a linked worktree for every role", async (t) => {
  const x = fixtures(t);
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    for (const r of READS) assert.equal(await call(f, `cd ${x.primary} && git ${r}`), undefined, `${role} ${r}`);
    assert.equal(await call(f, `git -C ${x.primary} worktree add $(mktemp -d) origin/main`), undefined, role);
    assert.equal(await call(f, `cd ${x.wt} && git merge origin/main`), undefined, role);
    assert.equal(await call(f, `git -C ${x.wt} checkout x`), undefined, role);
  }
});

test("the adapter judges a bare git command against the session cwd", async (t) => {
  const x = fixtures(t);
  const f = await started("lead");
  t.after(() => f.close());
  inCwd(f, x.primary);
  assert.equal((await call(f, "git checkout main"))?.block, true);
  inCwd(f, x.wt);
  assert.equal(await call(f, "git checkout main"), undefined);
});

test("the adapter judges only bash calls", async (t) => {
  const x = fixtures(t);
  const f = await started("lead");
  t.after(() => f.close());
  assert.equal(await call(f, `cd ${x.primary} && git checkout x`, "read"), undefined);
  assert.equal((await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: {} }))[0], undefined);
});

// ---- the skills ----

const skillFiles = (skill: string) => {
  const dir = new URL(`../skills/${skill}/`, import.meta.url);
  const refs = new URL("references/", dir);
  return [
    { name: "SKILL.md", text: readFileSync(new URL("SKILL.md", dir), "utf8") },
    ...readdirSync(refs).map((n) => ({ name: `references/${n}`, text: readFileSync(new URL(n, refs), "utf8") })),
  ];
};

const named = (skill: string) => {
  const text = skillFiles(skill)[0].text;
  for (const s of ["git fetch origin", "git show origin/main:<path>", "git log origin/main", "git diff origin/main", "git worktree add $(mktemp -d) origin/main"])
    assert.ok(text.includes(s), `${skill} SKILL.md lacks ${s}`);
};

test("the cpd-lead skill names the allowed ways to read the latest main", () => named("cpd-lead"));

test("the cpd-manager skill names the allowed ways to read the latest main", () => named("cpd-manager"));

test("no skill file names a git command that moves HEAD", () => {
  for (const skill of ["cpd-lead", "cpd-manager"])
    for (const f of skillFiles(skill)) assert.doesNotMatch(f.text, /git (checkout|switch|reset|pull|stash)\b|--detach/, `${skill}/${f.name}`);
});
