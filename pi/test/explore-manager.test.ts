import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { Fixture, shown } from "./fake.ts";

// ctx-explore, commands.md §15.22a: the explore-artefact guard is the Lead's AND the Manager's.
// A Manager explores before a Lead exists through its own Lead-less shaping Plan (air-d19); it gets the
// artefact's path in the message that wakes it (air-d20) and may not read the file either.
// Every test below exercises that new Manager half (it fails until the guard covers a Manager), alongside
// the case it is named for.

type Result = { block?: boolean; reason?: string } | undefined;

const REFUSAL = "φ: explore artefact: $CPD_HOME/run/<plan>/explore is for Workers; read the job-merged summary or add a follow-up scout Job";
const PREFIX = "φ: explore artefact:";

/** A started adapter of `role`: a Lead in Plan p1, a Manager of its Context (Lead-less Plan), CPD_JOB_KIND unset. */
async function started(t: TestContext, role: string) {
  const saved = process.env.CPD_JOB_KIND;
  t.after(() => {
    if (saved === undefined) delete process.env.CPD_JOB_KIND;
    else process.env.CPD_JOB_KIND = saved;
  });
  delete process.env.CPD_JOB_KIND;
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
  const tmp = mkdtempSync(join(tmpdir(), "explore-manager-"));
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

test("manager read of an explore artefact is refused", async (t) => {
  const f = await started(t, "manager");
  inCwd(f, plain(t));
  const abs = join(f.cpdHome, "run", "pay-4522", "explore", "look.md");
  refused(await call(f, "read", { path: abs }), "absolute");
  refused(await call(f, "read", { path: "~/.cpd/run/pay-4522/explore/look.md" }), "tilde");
  refused(await call(f, "read", { path: `@${abs}` }), "leading @");
  refused(await call(f, "read", { path: "$CPD_HOME/run/pay-4522/explore/look.md" }), "$CPD_HOME");
  refused(await call(f, "read", { path: "${CPD_HOME}/run/pay-4522/explore/look.md" }), "${CPD_HOME}");
});

test("manager bash file reader of an explore artefact is refused", async (t) => {
  const f = await started(t, "manager");
  inCwd(f, plain(t));
  const bash = (command: string) => call(f, "bash", { command, timeout: 10 });
  refused(await bash("cat $CPD_HOME/run/pay-4522/explore/look.md"), "cat");
  refused(await bash("sed -n 1,5p ~/.cpd/run/pay-4522/explore/look.md"), "sed");
  refused(await bash("grep -rn foo ${CPD_HOME}/run/pay-4522/explore"), "grep");
  refused(await bash(`head -n 20 ${join(f.cpdHome, "run", "pay-4522", "explore", "look.md")}`), "head, absolute");
});

test("manager cpd commands that name the explore path are allowed", async (t) => {
  const f = await started(t, "manager");
  inCwd(f, plain(t));
  const bash = (command: string) => call(f, "bash", { command, timeout: 10 });
  // First the guard's Manager half, so this test is red until it lands.
  refused(await bash("cat $CPD_HOME/run/pay-4522/explore/look.md"), "the guard is on for a Manager");
  // The Manager cites the artefact's path in a later Plan's brief and in cpd commands; only a file reader's operand is judged (§15.22a "What passes").
  assert.equal(await bash("cpd job add look2 --title t --tier H3 --spec /tmp/s.json --kind scout --plan pay-4522"), undefined, "a cpd command naming no path");
  assert.equal(await bash("cpd plan new pay-4600 --context ctx --title \"Next\" --shaping"), undefined, "the shaping plan new");
  assert.equal(await bash("cat > /tmp/s.json <<EOF\nread $CPD_HOME/run/pay-4522/explore/look.md\nEOF"), undefined, "a heredoc citing the path");
  assert.equal(await bash("printf '%s' 'see $CPD_HOME/run/pay-4522/explore/look.md' > /tmp/brief.md"), undefined, "writing the path into a brief");
});

test("manager may read other files", async (t) => {
  const f = await started(t, "manager");
  inCwd(f, plain(t));
  // First the guard's Manager half, so this test is red until it lands.
  refused(await call(f, "bash", { command: "cat $CPD_HOME/run/pay-4522/explore/look.md", timeout: 10 }), "the guard is on for a Manager");
  assert.equal(await call(f, "read", { path: join(f.cpdHome, "run", "pay-4522", "specs", "look.md") }), undefined, "another file under run/<plan>");
  assert.equal(await call(f, "read", { path: join(f.cpdHome, "run", "pay-4522", "explore-notes.md") }), undefined, "a sibling of the explore dir");
  assert.equal(await call(f, "read", { path: "/etc/hosts" }), undefined, "a file outside $CPD_HOME");
  assert.equal(await call(f, "bash", { command: "cat $CPD_HOME/run/pay-4522/specs/look.md", timeout: 10 }), undefined, "a file reader of another file");
});

test("lead read of an explore artefact is still refused", async (t) => {
  const f = await started(t, "lead");
  inCwd(f, plain(t));
  const abs = join(f.cpdHome, "run", "p1", "explore", "j1.md");
  refused(await call(f, "read", { path: abs }), "lead read");
  refused(await call(f, "bash", { command: `cat ${abs}`, timeout: 10 }), "lead bash reader");
  assert.equal(await call(f, "read", { path: join(f.cpdHome, "run", "p1", "specs", "j1.md") }), undefined, "a Spec");
  // The Lead's half is kept when the guard extends to a Manager: the same Plan's artefact stays refused for both.
  const m = await started(t, "manager");
  inCwd(m, plain(t));
  refused(await call(m, "read", { path: join(m.cpdHome, "run", "p1", "explore", "j1.md") }), "manager read of the same artefact");
});
