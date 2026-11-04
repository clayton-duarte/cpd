import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { Fixture, shown } from "./fake.ts";

// A3: a Lead may not read a Worker's explore artefact; Workers and Managers are untouched.

type Result = { block?: boolean; reason?: string } | undefined;

const REFUSAL = "φ: explore artefact: $CPD_HOME/run/<plan>/explore is for Workers; read the job-merged summary or add a follow-up scout Job";
const PREFIX = "φ: explore artefact:";

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

function plain(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "lead-explore-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const dir = join(tmp, "plain");
  mkdirSync(dir);
  return dir;
}

const refused = (r: Result, what: string) => {
  assert.equal(r?.block, true, what);
  assert.ok(r?.reason?.startsWith(PREFIX), `${what}: ${r?.reason}`);
  assert.ok(r?.reason?.includes(REFUSAL), `${what}: ${r?.reason}`);
};

test("a lead's read of an explore artefact is refused", async (t) => {
  const f = await started(t, "lead", undefined);
  inCwd(f, plain(t));
  const abs = join(f.cpdHome, "run", "p1", "explore", "j1.md");
  refused(await call(f, "read", { path: abs }), "absolute");
  refused(await call(f, "read", { path: "~/.cpd/run/p1/explore/j1.md" }), "tilde");
  refused(await call(f, "read", { path: `@${abs}` }), "leading @");
  assert.equal(await call(f, "read", { path: join(f.cpdHome, "run", "p1", "specs", "j1.md") }), undefined, "other file under run/<plan>");
  assert.equal(await call(f, "read", { path: "/etc/hosts" }), undefined, "repo/other file");
});

test("a lead's bash reader of an explore artefact is refused", async (t) => {
  const f = await started(t, "lead", undefined);
  inCwd(f, plain(t));
  const bash = (command: string) => call(f, "bash", { command, timeout: 10 });
  refused(await bash("cat $CPD_HOME/run/p/explore/j.md"), "cat");
  refused(await bash("sed -n 1,5p ~/.cpd/run/p/explore/j.md"), "sed");
  refused(await bash("grep -rn foo ${CPD_HOME}/run/p/explore"), "grep");

  assert.equal(await bash("cpd job add k --spec /tmp/s.json --kind scout"), undefined, "cpd command");
  assert.equal(await bash("cat > /tmp/s.json <<EOF\nread $CPD_HOME/run/p/explore/j.md\nEOF"), undefined, "heredoc citing the path");

  // a scout Worker's own explore profile decides, not the Lead guard
  const w = await started(t, "worker", "scout");
  inCwd(w, plain(t));
  const own = join(w.cpdHome, "run", "p1", "explore", "j1.md");
  assert.equal(await call(w, "bash", { command: `cat ${own}`, timeout: 10 }), undefined, "scout worker");
  const m = await started(t, "manager", undefined);
  inCwd(m, plain(t));
  assert.equal(await call(m, "bash", { command: "cat $CPD_HOME/run/p/explore/j.md", timeout: 10 }), undefined, "manager");
});
