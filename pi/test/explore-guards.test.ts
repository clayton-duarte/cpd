import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { test } from "node:test";
import { type ExploreKind, exploreKind, exploreVerdict } from "../lib/explore.ts";

// The explore guard profile (R11): pure pattern refusals keyed on CPD_JOB_KIND. Not an OS sandbox.

const HOME = "/fake/home/.cpd";
const EXPLORE = `${HOME}/run/plan-x/explore`;
const REPO = "/fake/repo";
const where = { cwd: EXPLORE, home: HOME, plan: "plan-x" };
const inRepo = (p: string) => p === REPO || p.startsWith(`${REPO}/`);
const KINDS: ExploreKind[] = ["scout", "research"];

const verdict = (kind: ExploreKind, name: string, args: Record<string, unknown>, cwd = EXPLORE) =>
  exploreVerdict(kind, { name, arguments: args }, { ...where, cwd }, inRepo);
const bash = (kind: ExploreKind, command: string, timeout: number | undefined = 20, cwd = EXPLORE) =>
  verdict(kind, "bash", timeout === undefined ? { command } : { command, timeout }, cwd);

const refused = (v: ReturnType<typeof exploreVerdict>, rule: string, what: string) => {
  assert.notEqual(v, undefined, `${what} must be refused`);
  assert.ok(v!.block.startsWith("φ: "), `${what}: reason starts with φ: (${v!.block})`);
  assert.ok(v!.block.includes(rule), `${what}: reason names ${rule} (${v!.block})`);
};
const allowed = (v: ReturnType<typeof exploreVerdict>, what: string) => assert.equal(v, undefined, `${what} must run (${v?.block})`);

test("scout refuses network commands in bash", () => {
  for (const c of [
    "curl https://example.com", "wget https://example.com", "ssh host ls", "scp a host:b", "sftp host", "nc host 80", "ftp host", "telnet host",
    "dig example.com", "nslookup example.com", "git fetch origin", "git pull", "git push", "git clone https://x/y", "git ls-remote origin",
    "gh pr list", "npm install", "pnpm add x", "yarn add x", "pip install x", "brew install x", "cargo install x", "cargo publish", "cargo search x",
    "echo hi && curl https://example.com", "/usr/bin/curl https://example.com",
  ]) refused(bash("scout", c), "network", c);
  allowed(bash("scout", "ls -la"), "ls");
  allowed(bash("scout", "cargo build"), "cargo build");
});

test("scout refuses mcp and web tools", () => {
  for (const n of ["mcp__github__search", "mcp__x", "web_search", "web_fetch", "websearch", "webfetch"]) refused(verdict("scout", n, {}), "network", n);
  allowed(verdict("scout", "read", { path: "/etc/hosts" }), "read");
});

test("research allows curl and web tools", () => {
  for (const c of ["curl https://example.com", "wget https://example.com", "gh api repos/x/y", "npm view left-pad", "git ls-remote origin"]) allowed(bash("research", c), c);
  for (const n of ["mcp__github__search", "web_search", "web_fetch"]) allowed(verdict("research", n, {}), n);
});

test("research refuses a literal token in argv", () => {
  for (const c of [
    "curl -H 'Authorization: Bearer abc123def' https://x",
    "curl -H 'Authorization: token ghp_abcdefghijklmnop' https://x",
    "curl https://x?k=github_pat_abcdefghijklmn",
    "curl -H 'x-api-key: sk-abcdefghijklmnop' https://x",
    "curl -H 'X: xoxb-123456789' https://x",
    "curl -H 'X: AKIAABCDEFGHIJKLMNOP' https://x",
    "curl -u alice:hunter2 https://x",
    "tool --password hunter2",
    "tool --token abcdef123",
    "tool --token=abcdef123",
  ]) refused(bash("research", c), "secret", c);
});

test("research allows a token read from stdin or a variable", () => {
  for (const c of [
    "curl -H \"Authorization: Bearer $TOKEN\" https://x",
    "curl -H \"Authorization: Bearer ${TOKEN}\" https://x",
    "tool --token \"$TOKEN\"",
    "tool --password \"$PW\"",
    "tool --token-stdin < /dev/null",
    "echo \"$TOKEN\" | tool --password-stdin",
    "curl -u \"$USER_PW\" https://x",
  ]) allowed(bash("research", c), c);
});

test("both kinds refuse edit and write outside the explore directory", () => {
  for (const k of KINDS) for (const name of ["edit", "write"]) {
    for (const p of [`${REPO}/src/a.ts`, "/fake/other/file", `${HOME}/run/other-plan/explore/a.md`, `${HOME}/run/plan-x/specs/a.md`, `${EXPLORE}/../x.md`, "../../escape.md"]) {
      refused(verdict(k, name, { path: p }), "write", `${k} ${name} ${p}`);
    }
  }
});

test("both kinds allow writing their own artefact", () => {
  for (const k of KINDS) for (const name of ["edit", "write"]) {
    for (const p of [`${EXPLORE}/out.md`, `${EXPLORE}/sub/out.md`, "out.md", `${tmpdir()}/scratch.txt`]) {
      allowed(verdict(k, name, { path: p }), `${k} ${name} ${p}`);
    }
  }
  allowed(bash("scout", `echo hi > ${EXPLORE}/out.md`), "redirect into the explore dir");
  allowed(bash("research", `echo hi | tee ${tmpdir()}/t.txt`), "tee into temp");
});

test("both kinds refuse unscoped find, grep -r and rg from the filesystem root or home", () => {
  for (const k of KINDS) for (const c of [
    "find / -name x", "find /Users -name x", "find /home -name x", "find ~ -name x", "find $HOME -name x", "find ${HOME} -name x",
    "grep -r foo /", "grep -rn foo /Users", "grep -R foo ~", "grep -r foo $HOME", "grep --recursive foo /home",
    "rg foo /", "rg foo /Users", "rg foo ~", "rg foo $HOME", "rg foo ${HOME}",
  ]) refused(bash(k, c), "scope", `${k} ${c}`);
});

test("both kinds allow scoped find, grep -r and rg", () => {
  for (const k of KINDS) for (const c of [
    `find ${REPO}/src -name x`, "find . -name x", "find /Users/someone/projects/app -name x", `grep -r foo ${REPO}/src`, "grep -rn foo .", "grep foo file.txt",
    `rg foo ${REPO}/src`, "rg foo", "rg foo ./src",
  ]) allowed(bash(k, c), `${k} ${c}`);
});

test("both kinds refuse a bash call without a timeout or over 30 seconds", () => {
  for (const k of KINDS) {
    refused(bash(k, "ls", undefined), "timeout", `${k} no timeout`);
    refused(bash(k, "ls", 31), "timeout", `${k} 31 s`);
    refused(bash(k, "ls", 600), "timeout", `${k} 600 s`);
    allowed(bash(k, "ls", 30), `${k} 30 s`);
    allowed(bash(k, "ls", 1), `${k} 1 s`);
  }
});

test("both kinds refuse a git or shell write inside a repository", () => {
  for (const k of KINDS) {
    for (const c of [
      `echo x > ${REPO}/a.txt`, `echo x >> ${REPO}/a.txt`, `echo x | tee ${REPO}/a.txt`,
      "git add .", "git commit -m x", "git checkout main", "git switch main", "git reset --hard", "git restore a", "git rebase main", "git merge x", "git pull",
      "git push", "git fetch", "git stash", "git clean -fd", "git apply p.diff", "git cherry-pick abc", "git revert abc", "git rm a", "git mv a b", "git worktree add ../w",
      `sed -i s/a/b/ ${REPO}/a.txt`, `rm ${REPO}/a.txt`, `mv ${REPO}/a ${REPO}/b`, `cp x ${REPO}/a`, `mkdir ${REPO}/d`, `touch ${REPO}/a`, `chmod +x ${REPO}/a`, `ln -s x ${REPO}/a`,
    ]) refused(bash(k, c, 20, REPO), "write", `${k} ${c}`);
    refused(bash(k, `rm ${REPO}/a.txt`), "write", `${k} rm into the repo from the explore dir`);
    allowed(bash(k, `cp ${REPO}/a ${EXPLORE}/a`), `${k} copy out of a repo`);
    allowed(bash(k, `mkdir ${EXPLORE}/d`), `${k} mkdir in the explore dir`);
  }
});

test("both kinds allow git log, show and diff", () => {
  for (const k of KINDS) for (const c of ["git log --oneline -5", "git show HEAD", "git diff main", "git status", "git blame a.ts", "cat a.ts | head", "ls -la", "git -C /fake/repo log"]) {
    allowed(bash(k, c, 20, REPO), `${k} ${c}`);
  }
});

test("an unknown CPD_JOB_KIND fails loud", () => {
  for (const v of ["lead", "Scout", "scouts", "worker", " scout", "none"]) {
    assert.throws(() => exploreKind({ CPD_JOB_KIND: v }), /CPD_JOB_KIND/, v);
  }
});

test("an unset CPD_JOB_KIND has no explore profile", () => {
  assert.equal(exploreKind({}), undefined);
  assert.equal(exploreKind({ CPD_JOB_KIND: undefined }), undefined);
  assert.equal(exploreKind({ CPD_JOB_KIND: "" }), undefined);
  assert.equal(exploreKind({ CPD_JOB_KIND: "scout" }), "scout");
  assert.equal(exploreKind({ CPD_JOB_KIND: "research" }), "research");
});
