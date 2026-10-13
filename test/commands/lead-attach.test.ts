import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { baseConfig, says } from "../herdr/setup.ts";
import { fakeBin, runCli, sandboxEnv } from "../helpers/index.ts";

const FIXTURE = new URL("../herdr/fixtures/session-finished.jsonl", import.meta.url).pathname;
const FORK = new URL("../herdr/fixtures/session-fork.jsonl", import.meta.url).pathname;
const SESSION_ID = "01a132ad-8b94-7785-9c0b-e72213509328";
const FORK_ID = "01a132ae-0000-7000-8000-000000000001";

const paneJson = (sessionPath: string | null) =>
  says({ id: "cli:pane:get", result: { type: "pane_info", pane: { pane_id: "w1:p1", workspace_id: "w1", ...(sessionPath ? { agent_session: { kind: "path", value: sessionPath } } : {}) } } });

function setup(sessionPath: string | null = FIXTURE, wfJson: object | null = { v: 1, id: "pay-1", title: "T", repo: "/r", lead: null, pr: null }) {
  const herdr = fakeBin("herdr", paneJson(sessionPath));
  const env = sandboxEnv({ HERDR_ENV: "1", HERDR_PANE_ID: "w1:p1" }, [herdr.dir]);
  mkdirSync(env.CPD_HOME!, { recursive: true });
  writeFileSync(join(env.CPD_HOME!, "config.json"), JSON.stringify({ ...baseConfig, herdrSession: "cpd-test" }));
  const wfDir = join(env.CPD_HOME!, "workflows", "pay-1");
  mkdirSync(wfDir, { recursive: true });
  if (wfJson) writeFileSync(join(wfDir, "workflow.json"), JSON.stringify(wfJson));
  return { herdr, env, wfDir, home: env.CPD_HOME! };
}

test("outside herdr: not_in_herdr, exit 3", async () => {
  const { env } = setup();
  delete env.HERDR_ENV;
  const r = await runCli(["lead", "attach", "--wf", "pay-1"], { env });
  assert.equal(r.code, 3);
  assert.equal(r.json.error.code, "not_in_herdr");
});

test("--wf (or CPD_WF) is required: workflow_required, exit 2; CPD_WF works", async () => {
  const { env, home } = setup();
  const r = await runCli(["lead", "attach"], { env });
  assert.equal(r.code, 2);
  assert.equal(r.json.error.code, "workflow_required");
  const ok = await runCli(["lead", "attach"], { env: { ...env, CPD_WF: "pay-1" } });
  assert.equal(ok.code, 0, ok.stdout + ok.stderr);
  assert.equal(readFileSync(join(home, "leads", SESSION_ID, "workflow"), "utf8"), "pay-1");
});

test("attach writes workflow.json.lead, the queue dir and leads/<id>/workflow; other keys are kept", async () => {
  const { herdr, env, wfDir, home } = setup();
  const r = await runCli(["lead", "attach", "--wf", "pay-1"], { env });
  assert.equal(r.code, 0, r.stdout + r.stderr);
  const lead = JSON.parse(readFileSync(join(wfDir, "workflow.json"), "utf8")).lead;
  assert.deepEqual(Object.keys(lead).sort(), ["attachedAt", "paneId", "queueDir", "sessionId", "sessionPath"]);
  assert.equal(lead.paneId, "w1:p1");
  assert.equal(lead.sessionPath, FIXTURE);
  assert.equal(lead.sessionId, SESSION_ID);
  assert.equal(lead.queueDir, join(home, "leads", SESSION_ID, "queue"));
  assert.ok(!Number.isNaN(Date.parse(lead.attachedAt)));
  assert.deepEqual(r.json, { ok: true, wf: "pay-1", lead });
  assert.ok(statSync(lead.queueDir).isDirectory());
  assert.equal(readFileSync(join(home, "leads", SESSION_ID, "workflow"), "utf8"), "pay-1");
  const wf = JSON.parse(readFileSync(join(wfDir, "workflow.json"), "utf8"));
  assert.deepEqual([wf.v, wf.id, wf.title, wf.repo, wf.pr], [1, "pay-1", "T", "/r", null]);
  assert.deepEqual(herdr.calls(), [["--session", "cpd-test", "pane", "get", "w1:p1"]]);
  const log = readFileSync(join(wfDir, "logs", "cpd.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  assert.equal(log.length, 1);
  assert.deepEqual([log[0].level, log[0].event, log[0].sessionId, log[0].replaced], ["info", "lead_attached", SESSION_ID, null]);
});

test("re-attaching from a new Lead session replaces the previous one and logs it", async () => {
  const { env, wfDir, home } = setup();
  await runCli(["lead", "attach", "--wf", "pay-1"], { env });
  // a second herdr pane/session
  const herdr2 = fakeBin("herdr", paneJson(FORK));
  const r = await runCli(["lead", "attach", "--wf", "pay-1"], { env: { ...env, PATH: `${herdr2.dir}:${env.PATH}` } });
  assert.equal(r.code, 0, r.stdout + r.stderr);
  const wf = JSON.parse(readFileSync(join(wfDir, "workflow.json"), "utf8"));
  assert.equal(wf.lead.sessionId, FORK_ID);
  assert.equal(readFileSync(join(home, "leads", FORK_ID, "workflow"), "utf8"), "pay-1");
  const log = readFileSync(join(wfDir, "logs", "cpd.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  assert.deepEqual(log.map((l) => l.replaced), [null, SESSION_ID]);
});

test("refusals: unknown workflow is exit 4; a pane without an agent session and a missing session file fail loud", async () => {
  const a = setup();
  const r1 = await runCli(["lead", "attach", "--wf", "nope-1"], { env: a.env });
  assert.deepEqual([r1.code, r1.json.error.code], [4, "workflow_not_found"]);
  assert.equal(existsSync(join(a.home, "workflows", "nope-1")), false);
  const b = setup(null);
  const r2 = await runCli(["lead", "attach", "--wf", "pay-1"], { env: b.env });
  assert.deepEqual([r2.code, r2.json.error.code], [1, "lead_session_unknown"]);
  const c = setup("/nonexistent/session.jsonl");
  const r3 = await runCli(["lead", "attach", "--wf", "pay-1"], { env: c.env });
  assert.deepEqual([r3.code, r3.json.error.code], [4, "session_not_found"]);
  assert.equal(existsSync(join(c.home, "leads")), false);
  const d = setup();
  const r4 = await runCli(["lead", "attach", "--wf", "Bad_ID"], { env: d.env });
  assert.deepEqual([r4.code, r4.json.error.code], [2, "invalid_workflow_id"]);
});

test("a workflow.json that is not valid fails loud and is left untouched", async () => {
  const { env, wfDir } = setup(FIXTURE, { v: 2, id: "pay-1" });
  const r = await runCli(["lead", "attach", "--wf", "pay-1"], { env });
  assert.deepEqual([r.code, r.json.error.code], [1, "workflow_corrupt"]);
  assert.deepEqual(JSON.parse(readFileSync(join(wfDir, "workflow.json"), "utf8")), { v: 2, id: "pay-1" });
});
