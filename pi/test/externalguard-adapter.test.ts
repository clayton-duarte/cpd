import assert from "node:assert/strict";
import { beforeEach, test } from "node:test";
import { grantSlackSend, reasonText, resetSlackGrants } from "../lib/externalguard.ts";
import { Fixture, shown } from "./fake.ts";

// Decision pro-d451: the adapter's tool_call handler refuses the external writes for every role with a run.

type Verdict = { block?: boolean; reason?: string } | undefined;
const SEND = "mcp__slack__slack_send_message";
const ROLES = ["manager", "lead", "worker"];

beforeEach(() => resetSlackGrants());

async function started(role: string) {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  return f;
}
const fire = async (f: Fixture, toolName: string, input: unknown, nested = false, id = "p"): Promise<Verdict> =>
  (await f.pi.fireWith("tool_call", nested ? { toolName, toolCallId: `${id}/1`, parentToolCallId: id, input } : { toolName, toolCallId: id, input }))[0] as Verdict;
const oneLine = (v: Verdict, what: string) => {
  assert.equal(v?.block, true, `${what}: refused`);
  assert.ok(typeof v?.reason === "string" && v.reason !== "" && !v.reason.includes("\n"), `${what}: one-line reason`);
  assert.ok(v.reason.includes("cpd_external_approve"), `${what}: names the tool`);
};

test("the adapter refuses raw gh comment writes for manager, lead and worker", async (t) => {
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    for (const command of ["gh pr comment 1 -b x", "timeout 30 gh pr review 1 --approve", "gh api repos/o/r/issues/1/comments -f body=x"]) {
      const v = await fire(f, "bash", { command });
      oneLine(v, `${role} ${command}`);
      assert.equal(v?.reason, (await fire(f, "bash", { command }, true))?.reason, "nested is the same");
    }
    assert.equal((await fire(f, "bash", { command: "gh pr view 1", timeout: 5 }))?.block, undefined, `${role}: a read runs`);
  }
});

test("the adapter refuses a direct slack send until a one-shot grant and lets exactly one through", async (t) => {
  for (const role of ["manager", "lead"]) {
    const f = await started(role);
    t.after(() => f.close());
    const input = { channel_id: "C1", message: "hi" };
    oneLine(await fire(f, SEND, input), `${role} ungranted`);
    oneLine(await fire(f, "mcp__slack__slack_send_message_draft", input), `${role} draft`);
    grantSlackSend("slack_send_message", { channel: "C1", message: "hi" });
    assert.equal(await fire(f, SEND, input), undefined, `${role}: granted passes`);
    oneLine(await fire(f, SEND, input), `${role} second`);
  }
  const w = await started("worker");
  t.after(() => w.close());
  grantSlackSend("slack_send_message", { channel: "C1", message: "hi" });
  oneLine(await fire(w, SEND, { channel_id: "C1", message: "hi" }), "worker is refused even with a grant");
});

test("the adapter refuses a nested slack send of a codemode script", async (t) => {
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    const input = { channel_id: "C1", message: "hi" };
    oneLine(await fire(f, SEND, input, true), `${role} nested`);
    const script = { code: `await tools.${SEND}({ channel_id: "C1", message: "hi" })` };
    assert.equal(await fire(f, "codemode", script), undefined, `${role}: the script itself is judged by its nested calls`);
  }
  assert.ok(reasonText("x").includes("cpd_external_approve"));
});

test("the adapter refuses cpd pr approve and slack-post.ts in bash", async (t) => {
  for (const role of ROLES) {
    const f = await started(role);
    t.after(() => f.close());
    for (const command of ["cpd pr approve --kind post --workflow w", "timeout 5 cpd pr approve --kind reply --workflow w --to c", "node /x/slack-post.ts --channel C1", "/x/slack-post.ts hi"]) {
      oneLine(await fire(f, "bash", { command }), `${role} ${command}`);
      oneLine(await fire(f, "bash", { command }, true), `${role} nested ${command}`);
    }
  }
});
