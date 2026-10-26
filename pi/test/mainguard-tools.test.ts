import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { primaryCheckoutWrite, reasonText } from "../lib/mainguard.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.16, the `edit` and `write` half: no agent changes the files of a repo's primary checkout.

const PREFIX = "φ: no agent moves HEAD or changes the files of a repo's primary checkout";
const ALTERNATIVES = ["git show origin/main:<path>", "git log origin/main", "git diff origin/main", "git worktree add $(mktemp -d) origin/main"];
const WORKTREE_WAY = "git worktree add -b <branch> <dir> origin/main";

type Fx = { tmp: string; primary: string; wt: string; inside: string; plain: string; missing: string };

/** The file system of §15.16: a primary checkout (`.git` directory), a linked worktree beside it, a linked worktree inside it, a plain directory and a missing path; no git runs. */
function fixtures(t: TestContext): Fx {
  const tmp = mkdtempSync(join(tmpdir(), "mainguard-tools-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(primary, "src"));
  mkdirSync(join(primary, "docs"));
  writeFileSync(join(primary, "docs", "commands.md"), "x");
  const wt = join(tmp, "projects", "repo.wt");
  mkdirSync(join(wt, "src"), { recursive: true });
  writeFileSync(join(wt, ".git"), "gitdir: ../repo/.git/worktrees/repo.wt\n");
  const inside = join(primary, "inside");
  mkdirSync(join(inside, "src"), { recursive: true });
  writeFileSync(join(inside, ".git"), "gitdir: ../../repo/.git/worktrees/inside\n");
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  return { tmp, primary, wt, inside, plain, missing: join(tmp, "does-not-exist") };
}

test("refuses edit and write of a path in a primary checkout, absolute, tilde, at-prefixed and relative", (t) => {
  const x = fixtures(t);
  const abs = join(x.primary, "docs", "commands.md");
  for (const tool of ["edit", "write"] as const) {
    const want = `${tool} ${abs} in ${x.primary}`;
    assert.equal(primaryCheckoutWrite(tool, { path: abs }, x.plain, x.tmp), want, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: "~/projects/repo/docs/commands.md" }, x.plain, x.tmp), want, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: "@~/projects/repo/docs/commands.md" }, x.plain, x.tmp), want, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: "docs/commands.md" }, x.primary, x.tmp), want, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: "../docs/commands.md" }, join(x.primary, "src"), x.tmp), want, tool);
    const config = join(x.primary, ".git", "config");
    assert.equal(primaryCheckoutWrite(tool, { path: config }, x.plain, x.tmp), `${tool} ${config} in ${x.primary}`, tool);
  }
});

test("refuses edit and write of a new file below a directory that does not exist yet in a primary checkout", (t) => {
  const x = fixtures(t);
  const abs = join(x.primary, "new", "dir", "f.md");
  for (const tool of ["edit", "write"] as const)
    assert.equal(primaryCheckoutWrite(tool, { path: abs }, x.plain, x.tmp), `${tool} ${abs} in ${x.primary}`, tool);
});

test("passes edit and write in a linked worktree, outside a repository, for another tool and with no usable path", (t) => {
  const x = fixtures(t);
  const primaryPath = join(x.primary, "docs", "commands.md");
  for (const tool of ["edit", "write"] as const) {
    assert.equal(primaryCheckoutWrite(tool, { path: join(x.wt, "docs", "x.md") }, x.plain, x.tmp), undefined, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: join(x.inside, "src", "x.ts") }, x.plain, x.tmp), undefined, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: join(x.plain, "x") }, x.plain, x.tmp), undefined, tool);
    assert.equal(primaryCheckoutWrite(tool, { path: join(x.missing, "x") }, x.plain, x.tmp), undefined, tool);
    for (const input of [{}, { path: "" }, { path: 3 }, null, undefined])
      assert.equal(primaryCheckoutWrite(tool, input, x.plain, x.tmp), undefined, `${tool} ${JSON.stringify(input)}`);
  }
  for (const tool of ["read", "bash", "grep"] as const) assert.equal(primaryCheckoutWrite(tool, { path: primaryPath }, x.plain, x.tmp), undefined, tool);
});

test("the edit and write reason names the tool, the path, the root and the worktree way", () => {
  const r = reasonText("edit /p/cpd/docs/x.md in /p/cpd");
  assert.ok(!r.includes("\n"), r);
  assert.ok(r.startsWith(PREFIX), r);
  for (const a of [...ALTERNATIVES, WORKTREE_WAY]) assert.ok(r.includes(a), a);
  assert.ok(r.endsWith("refused: edit /p/cpd/docs/x.md in /p/cpd"), r);
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
const call = async (f: Fixture, toolName: "edit" | "write" | "bash" | "read", input: unknown) =>
  (await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input }))[0] as Result;
const inCwd = (f: Fixture, dir: string) => {
  const ctx = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => ({ ...ctx(), cwd: dir });
};

const ROLES = ["lead", "manager", "worker"];

test("the adapter blocks edit, write and a bash write in a primary checkout for a Lead, a Manager and a Worker", async (t) => {
  const x = fixtures(t);
  const doc = join(x.primary, "docs", "commands.md");
  const target = join(x.primary, "f");
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    inCwd(f, x.plain);
    for (const [tool, matched] of [
      ["edit", `edit ${doc} in ${x.primary}`],
      ["write", `write ${doc} in ${x.primary}`],
    ] as const) {
      const r = await call(f, tool, { path: doc });
      assert.equal(r?.block, true, `${role} ${tool}`);
      assert.equal(r?.reason, reasonText(matched), `${role} ${tool}`);
    }
    for (const [command, matched] of [
      [`echo x > ${target}`, `> ${target} in ${x.primary}`],
      [`git -C ${x.primary} commit -m x`, `git commit in ${x.primary}`],
    ] as const) {
      const r = await call(f, "bash", { command });
      assert.equal(r?.block, true, `${role} ${command}`);
      assert.equal(r?.reason, reasonText(matched), `${role} ${command}`);
    }
  }
});

test("the adapter passes edit, write and bash writes in a linked worktree for every role", async (t) => {
  const x = fixtures(t);
  const wtDoc = join(x.wt, "docs", "x.md");
  const wtFile = join(x.wt, "f");
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    inCwd(f, x.plain);
    assert.equal(await call(f, "edit", { path: wtDoc }), undefined, role);
    assert.equal(await call(f, "write", { path: wtDoc }), undefined, role);
    assert.equal(await call(f, "bash", { command: `echo x > ${wtFile}` }), undefined, role);
    assert.equal(await call(f, "bash", { command: `rm ${wtFile}` }), undefined, role);
    assert.equal(await call(f, "read", { path: join(x.primary, "docs", "commands.md") }), undefined, role);
  }
});
