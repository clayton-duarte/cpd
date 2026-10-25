import assert from "node:assert/strict";
import { test, type TestContext } from "node:test";
import { hostApproveBypass } from "../lib/hostapprove.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.<n>: the approval is the User's, so no agent runs `cpd host approve`; the adapter refuses it.
// `cpd host repair` and every other cpd command pass. The §15.<n> number is the docs' placeholder.

type Result = { block?: boolean; reason?: string } | undefined;

const REASON = /^cpd: approvals are the User's: ask with the tool cpd_host_approve \(commands\.md §15\.[^)]+\); refused: cpd host approve$/;
const REFUSED = ["cpd host approve", "cpd host approve --reason dirty", "cd ~/projects/cpd && cpd host approve", "FOO=1 cpd host approve"];
const PASSING = ["cpd host repair", "cpd host repair --reason dirty", "cpd host", "host approve", "echo cpd host approve", "cpd secret get x --vault cpd --field f", "cpd session show"];

/** A Manager adapter on the fake, and how it judges a `bash` call. */
async function manager(t: TestContext) {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return (command: string) => f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command } }).then((out) => out[0] as Result);
}

test("bash from an agent cannot run cpd host approve", async (t) => {
  const call = await manager(t);
  for (const command of REFUSED) {
    const r = await call(command);
    assert.equal(r?.block, true, command);
    assert.match(String(r?.reason), REASON, command);
  }
});

test("cpd host repair and other cpd commands are not refused", async (t) => {
  const call = await manager(t);
  // The guard is live: it refuses the approval ...
  assert.equal((await call("cpd host approve"))?.block, true);
  // ... and passes the repair and every other command.
  for (const command of PASSING) {
    assert.equal(hostApproveBypass(command), undefined, command);
    assert.equal(await call(command), undefined, command);
  }
});
