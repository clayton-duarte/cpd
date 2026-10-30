import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { CpdError } from "../lib/cpd.ts";
import { exploreVerdict } from "../lib/explore.ts";
import { parseConfig } from "../durable-worker/config.ts";
import { workerVerdict } from "../durable-worker/guards.ts";
import { Fixture, shown } from "./fake.ts";

// R11: the explore profile of CPD_JOB_KIND is applied by the pi adapter and by the durable runner, to Workers only.

type Result = { block?: boolean; reason?: string } | undefined;

/** A started adapter of `role` in Plan p1, CPD_JOB_KIND set to `kind` (unset when undefined) for the test's length. */
async function started(t: TestContext, role: string, kind: string | undefined) {
  const saved = process.env.CPD_JOB_KIND;
  t.after(() => {
    if (saved === undefined) delete process.env.CPD_JOB_KIND;
    else process.env.CPD_JOB_KIND = saved;
  });
  if (kind === undefined) delete process.env.CPD_JOB_KIND;
  else process.env.CPD_JOB_KIND = kind;
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  return f;
}

const call = async (f: Fixture, toolName: string, input: unknown) =>
  (await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input }))[0] as Result;

const inCwd = (f: Fixture, dir: string) => {
  const ctx = f.pi.ctx.bind(f.pi);
  f.pi.ctx = () => ({ ...ctx(), cwd: dir });
};

/** A plain directory and a repository (a `.git` directory), no git runs. */
function dirs(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "explore-wiring-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const plain = join(tmp, "plain");
  const repo = join(tmp, "repo");
  mkdirSync(plain);
  mkdirSync(join(repo, ".git"), { recursive: true });
  return { tmp, plain, repo };
}

const blocked = (r: Result, rule: string, what: string) => {
  assert.equal(r?.block, true, what);
  assert.ok(r?.reason?.startsWith("φ: "), `${what}: ${r?.reason}`);
  assert.ok(r?.reason?.includes(rule), `${what}: ${r?.reason}`);
};

test("the adapter applies the explore profile of CPD_JOB_KIND to a worker, for bash and for other tools", async (t) => {
  const x = dirs(t);
  for (const kind of ["scout", "research"] as const) {
    const f = await started(t, "worker", kind);
    inCwd(f, x.plain);
    const explore = join(f.cpdHome, "run", "p1", "explore");
    blocked(await call(f, "bash", { command: "ls", timeout: 10 + 25 }), "timeout", `${kind} over bound`);
    blocked(await call(f, "bash", { command: "ls" }), "timeout", `${kind} no timeout`);
    blocked(await call(f, "bash", { command: `echo x > ${join(x.repo, "f")}`, timeout: 10 }), "write", `${kind} redirect`);
    blocked(await call(f, "edit", { path: join(x.repo, "f") }), "write", `${kind} edit outside`);
    blocked(await call(f, "write", { path: join(x.repo, "f") }), "write", `${kind} write outside`);
    assert.equal(await call(f, "write", { path: join(explore, "a.md") }), undefined, `${kind} write in explore dir`);
    assert.equal(await call(f, "bash", { command: "ls", timeout: 10 }), undefined, `${kind} ls`);
  }
  const s = await started(t, "worker", "scout");
  inCwd(s, x.plain);
  blocked(await call(s, "bash", { command: "curl https://example.com", timeout: 10 }), "network", "scout curl");
  blocked(await call(s, "mcp__docs__search", {}), "network", "scout mcp tool");
  const r = await started(t, "worker", "research");
  inCwd(r, x.plain);
  assert.equal(await call(r, "mcp__docs__search", {}), undefined, "research mcp tool");
  assert.equal(await call(r, "bash", { command: "curl https://example.com", timeout: 10 }), undefined, "research curl");
  blocked(await call(r, "bash", { command: "curl -H 'Authorization: Bearer totally_fake_curl_bearer_secret' https://example.com", timeout: 10 }), "secret", "research secret");
});

test("the adapter applies no explore profile to a lead or a manager", async (t) => {
  const x = dirs(t);
  // control: the same call by a scout Worker is refused, so the passes below are the role's, not a missing profile
  const w = await started(t, "worker", "scout");
  inCwd(w, x.plain);
  blocked(await call(w, "bash", { command: "curl https://example.com", timeout: 10 }), "network", "scout worker control");
  for (const role of ["lead", "manager"]) {
    const f = await started(t, role, "scout");
    inCwd(f, x.plain);
    assert.equal(await call(f, "mcp__docs__search", {}), undefined, `${role} mcp tool`);
    assert.equal(await call(f, "edit", { path: join(x.plain, "f") }), undefined, `${role} edit outside explore dir`);
    assert.equal(await call(f, "bash", { command: "curl https://example.com", timeout: 10 }), undefined, `${role} curl`);
  }
});

test("the adapter applies no explore profile to a worker without CPD_JOB_KIND", async (t) => {
  const x = dirs(t);
  // control: the same call by a scout Worker is refused, so the passes below are the role's, not a missing profile
  const w = await started(t, "worker", "scout");
  inCwd(w, x.plain);
  blocked(await call(w, "bash", { command: "curl https://example.com", timeout: 10 }), "network", "scout worker control");
  for (const kind of [undefined, ""]) {
    const f = await started(t, "worker", kind);
    inCwd(f, x.plain);
    assert.equal(await call(f, "mcp__docs__search", {}), undefined, `kind ${JSON.stringify(kind)} mcp tool`);
    assert.equal(await call(f, "edit", { path: join(x.plain, "f") }), undefined, `kind ${JSON.stringify(kind)} edit`);
    assert.equal(await call(f, "bash", { command: "curl https://example.com" }), undefined, `kind ${JSON.stringify(kind)} curl, no timeout`);
  }
});

test("the durable workerVerdict applies the same explore profile", (t) => {
  const x = dirs(t);
  const inside = (p: string) => p === x.repo || p.startsWith(`${x.repo}/`);
  const calls = [
    { name: "bash", arguments: { command: "ls" } },
    { name: "bash", arguments: { command: "ls", timeout: 10 } },
    { name: "bash", arguments: { command: `echo x > ${join(x.repo, "f")}`, timeout: 10 } },
    { name: "bash", arguments: { command: "curl https://example.com", timeout: 10 } },
    { name: "bash", arguments: { command: "curl -H 'Authorization: Bearer totally_fake_curl_bearer_secret' https://example.com", timeout: 10 } },
    { name: "edit", arguments: { path: join(x.repo, "f") } },
    { name: "write", arguments: { path: join(x.tmp, "home", "run", "p1", "explore", "a.md") } },
    { name: "mcp__docs__search", arguments: {} },
  ];
  for (const kind of ["scout", "research"] as const) {
    for (const c of calls) {
      const where = { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "home"), plan: "p1", kind };
      const want = exploreVerdict(kind, c, { cwd: where.cwd, home: where.cpdHome, plan: "p1" }, inside);
      assert.deepEqual(workerVerdict(c, where), want, `${kind} ${JSON.stringify(c)}`);
    }
  }
  assert.notEqual(workerVerdict(calls[0]!, { cwd: x.plain, home: x.tmp, cpdHome: join(x.tmp, "home"), plan: "p1", kind: "scout" }), undefined, "an unbounded bash call is refused");
});

test("the durable config reads CPD_JOB_KIND and refuses an unknown value", () => {
  const ARGV = ["--spec", "/s.md", "--model", "m"];
  const BASE = { CPD_HOME: "/h", CPD_SESSION: "s", CPD_PLAN: "p", CPD_ATTEMPT: "a" };
  assert.equal(parseConfig(ARGV, { ...BASE, CPD_JOB_KIND: "scout" }, "/wt").jobKind, "scout");
  assert.equal(parseConfig(ARGV, { ...BASE, CPD_JOB_KIND: "research" }, "/wt").jobKind, "research");
  assert.equal(parseConfig(ARGV, BASE, "/wt").jobKind, undefined);
  assert.equal(parseConfig(ARGV, { ...BASE, CPD_JOB_KIND: "" }, "/wt").jobKind, undefined);
  assert.throws(
    () => parseConfig(ARGV, { ...BASE, CPD_JOB_KIND: "docs" }, "/wt"),
    (e: unknown) => e instanceof CpdError && e.code === "usage" && e.message.includes("CPD_JOB_KIND"),
  );
});
