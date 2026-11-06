import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeEach, test, type TestContext } from "node:test";
import { workerVerdict } from "../../durable-worker/guards.ts";
import { grantSlackSend, resetSlackGrants } from "../../lib/externalguard.ts";

// Decision pro-d451 in the durable worker: workerVerdict.

beforeEach(() => resetSlackGrants());
function whereOf(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "durable-worker-external-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  return { cwd: tmp, home: tmp, cpdHome: join(tmp, "cpd") };
}
const blocked = (v: unknown, what: string) => {
  const b = (v as { block?: unknown } | undefined)?.block;
  assert.ok(typeof b === "string" && b !== "" && !b.includes("\n") && b.includes("cpd_external_approve"), `${what}: ${JSON.stringify(v)}`);
};

test("the durable worker refuses raw gh comment writes", (t) => {
  const where = whereOf(t);
  for (const command of ["gh pr comment 1 -b x", "timeout 30 gh pr review 1 --approve", "gh api repos/o/r/issues/1/comments -f body=x"]) {
    blocked(workerVerdict({ name: "bash", arguments: { command } }, where), command);
  }
  assert.equal(workerVerdict({ name: "bash", arguments: { command: "gh pr view 1" } }, where), undefined);
});

test("the durable worker refuses every slack send", (t) => {
  const where = whereOf(t);
  for (const name of ["mcp__slack__slack_send_message", "mcp__slack__slack_send_message_draft"]) {
    const call = { name, arguments: { channel_id: "C1", message: "hi" } };
    blocked(workerVerdict(call, where), name);
    grantSlackSend("slack_send_message", { channel: "C1", message: "hi" });
    grantSlackSend("slack_send_message_draft", { channel: "C1", message: "hi" });
    blocked(workerVerdict(call, where), `${name} even with a grant`);
    resetSlackGrants();
  }
});

test("the durable worker refuses cpd pr approve and slack-post.ts in bash", (t) => {
  const where = whereOf(t);
  for (const command of ["cpd pr approve --kind post --workflow w", "node /x/slack-post.ts --channel C1", "/x/slack-post.ts hi"]) {
    blocked(workerVerdict({ name: "bash", arguments: { command } }, where), command);
  }
});
