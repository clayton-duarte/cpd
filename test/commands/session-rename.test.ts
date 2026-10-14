import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { LeadMessageSchema } from "../../src/queue/messages.ts";
import { runCli, sandboxEnv } from "../helpers/index.ts";

const FIXTURE = new URL("../herdr/fixtures/session-finished.jsonl", import.meta.url).pathname;
const SESSION_ID = "01a132ad-8b94-7785-9c0b-e72213509328";

const queueOf = (env: NodeJS.ProcessEnv, id = SESSION_ID) => join(env.CPD_HOME!, "leads", id, "queue");
const files = (dir: string) => (existsSync(dir) ? readdirSync(dir) : []);

function oneMessage(env: NodeJS.ProcessEnv) {
  const names = files(queueOf(env));
  assert.equal(names.length, 1);
  return LeadMessageSchema.parse(JSON.parse(readFileSync(join(queueOf(env), names[0]!), "utf8")));
}

test("PI_SESSION_FILE: one valid rename message in that session's queue", async () => {
  const env = sandboxEnv({ PI_SESSION_FILE: FIXTURE });
  const r = await runCli(["session", "rename", "my name"], { env });
  assert.equal(r.code, 0);
  assert.deepEqual(r.json, { ok: true, sessionId: SESSION_ID, name: "my name" });
  const m = oneMessage(env);
  assert.equal(m.kind, "rename");
  assert.deepEqual(m.data, { name: "my name" });
});

test("--session <path> wins over PI_SESSION_FILE", async () => {
  const env = sandboxEnv({ PI_SESSION_FILE: "/nonexistent/other.jsonl" });
  const r = await runCli(["session", "rename", "x", "--session", FIXTURE], { env });
  assert.equal(r.code, 0);
  assert.equal(r.json.sessionId, SESSION_ID);
  assert.equal(oneMessage(env).kind, "rename");
});

test("--session <id> of a session with a cpd dir", async () => {
  const env = sandboxEnv();
  mkdirSync(queueOf(env), { recursive: true });
  const r = await runCli(["session", "rename", "by id", "--session", SESSION_ID], { env });
  assert.equal(r.code, 0);
  assert.deepEqual(oneMessage(env).data, { name: "by id" });
});

test("--session <id> unknown: session_unknown, nothing written", async () => {
  const env = sandboxEnv();
  const r = await runCli(["session", "rename", "x", "--session", "nope"], { env });
  assert.deepEqual([r.code, r.json.error.code], [3, "session_unknown"]);
  assert.equal(existsSync(join(env.CPD_HOME!, "leads")), false);
});

test("no session: session_unknown, exit 3", async () => {
  const env = sandboxEnv();
  const r = await runCli(["session", "rename", "x"], { env });
  assert.deepEqual([r.code, r.json.error.code], [3, "session_unknown"]);
});

for (const [label, name] of [["empty", ""], ["81 characters", "a".repeat(81)], ["control character", "a\nb"]] as const) {
  test(`invalid name (${label}): refused, nothing written`, async () => {
    const env = sandboxEnv({ PI_SESSION_FILE: FIXTURE });
    const r = await runCli(["session", "rename", name], { env });
    assert.deepEqual([r.code, r.json.error.code], [2, "invalid_name"]);
    assert.deepEqual(files(queueOf(env)), []);
  });
}

test("80 characters is accepted; a missing name is a usage error", async () => {
  const env = sandboxEnv({ PI_SESSION_FILE: FIXTURE });
  assert.equal((await runCli(["session", "rename", "a".repeat(80)], { env })).code, 0);
  assert.equal((await runCli(["session", "rename"], { env })).json.error.code, "usage");
});
