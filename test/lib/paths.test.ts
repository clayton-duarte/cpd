import { test } from "node:test";
import assert from "node:assert/strict";
import { join } from "node:path";
import {
  agentName, assertAttemptId, assertJobKey, assertWorkflowId, attemptBranch, attemptId, attemptLogPath, cpdHome,
  escalationAnswerPath, escalationId, escalationQuestionPath, leadBranch, leadQueueDir, ledgerDir, ledgerLockPath,
  mergeLockPath, slotPath, specPath, workflowDir,
} from "../../src/lib/paths.ts";
import { sandboxEnv } from "../helpers/index.ts";

const home = sandboxEnv().CPD_HOME!;
process.env.CPD_HOME = home;

const code = (c: string, exit = 2) => (e: any) => e.code === c && e.exitCode === exit;

test("workflow ids: §3 pattern", () => {
  for (const ok of ["pay-4521", "ab", "a" + "b".repeat(23)]) assertWorkflowId(ok);
  for (const bad of ["a", "A1", "1ab", "ab_c", "a" + "b".repeat(24), "../x", ""]) {
    assert.throws(() => assertWorkflowId(bad), code("invalid_workflow_id"));
  }
});

test("job keys: §3 pattern", () => {
  for (const ok of ["a", "api-schema", "a" + "b".repeat(31)]) assertJobKey(ok);
  for (const bad of ["", "A", "1a", "a_b", "a" + "b".repeat(32), "a/b"]) assert.throws(() => assertJobKey(bad), code("invalid_job_key"));
});

test("attempt ids and branches", () => {
  assert.equal(attemptId("api-schema", 2), "api-schema-a2");
  assert.throws(() => attemptId("api-schema", 0), code("invalid_attempt_number"));
  assert.throws(() => attemptId("Bad", 1), code("invalid_job_key"));
  assertAttemptId("api-schema-a12");
  assert.throws(() => assertAttemptId("api-schema-a0"), code("invalid_attempt_id"));
  assert.throws(() => assertAttemptId("api-schema"), code("invalid_attempt_id"));
  assert.equal(leadBranch("pay-4521"), "cpd/pay-4521");
  assert.equal(attemptBranch("pay-4521", "api-schema-a2"), "cpd/pay-4521--api-schema-a2");
  assert.equal(escalationId("api-schema-a2", 1), "api-schema-a2-q1");
});

test("agent names: c + 12 hex of sha1(wf/attempt/role), stable and role-specific", () => {
  const w = agentName("pay-4521", "api-schema-a2", "w");
  assert.match(w, /^c[0-9a-f]{12}$/);
  assert.equal(w, agentName("pay-4521", "api-schema-a2", "w"));
  assert.notEqual(w, agentName("pay-4521", "api-schema-a2", "r"));
  assert.notEqual(w, agentName("pay-4522", "api-schema-a2", "w"));
  // sha1("pay-4521/api-schema-a2/w") `printf pay-4521/api-schema-a2/w | shasum`
  assert.equal(w, "c36959c17264b");
  assert.throws(() => agentName("pay-4521", "api-schema-a2", "x" as any), code("invalid_agent_role"));
});

test("state paths follow §4 under CPD_HOME", () => {
  assert.equal(cpdHome(), home);
  assert.equal(workflowDir("pay-4521"), join(home, "workflows/pay-4521"));
  assert.equal(ledgerDir("pay-4521"), join(home, "workflows/pay-4521/ledger"));
  assert.equal(ledgerLockPath("pay-4521"), join(home, "workflows/pay-4521/locks/ledger.lock"));
  assert.equal(specPath("pay-4521", "api-schema-a2"), join(home, "workflows/pay-4521/specs/api-schema-a2.md"));
  assert.equal(attemptLogPath("pay-4521", "api-schema-a2"), join(home, "workflows/pay-4521/logs/attempt-api-schema-a2.log"));
  assert.equal(escalationQuestionPath("pay-4521", "api-schema-a2-q1"), join(home, "workflows/pay-4521/escalations/api-schema-a2-q1.q.json"));
  assert.equal(escalationAnswerPath("pay-4521", "api-schema-a2-q1"), join(home, "workflows/pay-4521/escalations/api-schema-a2-q1.a.json"));
  assert.equal(leadQueueDir("0190-abc"), join(home, "leads/0190-abc/queue"));
  assert.equal(slotPath(1), join(home, "slots/slot-1.json"));
  assert.match(mergeLockPath("/repo"), new RegExp(`^${home}/locks/merge-[0-9a-f]{40}\\.lock$`));
});

test("path functions reject traversal", () => {
  assert.throws(() => workflowDir("../x"), code("invalid_workflow_id"));
  assert.throws(() => leadQueueDir("../x"), code("invalid_session_id"));
  assert.throws(() => specPath("pay-4521", "../../x"), code("invalid_attempt_id"));
});

test("cpdHome defaults to ~/.cpd when CPD_HOME is unset", () => {
  const saved = process.env.CPD_HOME;
  delete process.env.CPD_HOME;
  try {
    assert.match(cpdHome(), /\/\.cpd$/);
  } finally {
    process.env.CPD_HOME = saved;
  }
});
