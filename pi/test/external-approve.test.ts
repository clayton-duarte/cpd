import assert from "node:assert/strict";
import { test, type TestContext } from "node:test";
import { registerExternalApproveTool } from "../lib/externalapprove.ts";
import { takeSlackSend, resetSlackGrants } from "../lib/externalguard.ts";
import { createHost, type Host } from "../lib/host.ts";
import type { Run } from "../lib/run.ts";
import { Fixture } from "./fake.ts";

// Decision pro-d451: the tool cpd_external_approve, driven through the fake host (model: host-approve.test.ts).

function setup(t: TestContext, role: string | undefined, ui = true) {
  resetSlackGrants();
  const f = new Fixture();
  t.after(() => f.close());
  const host: Host = createHost(f.pi, { env: process.env });
  if (role !== undefined) host.run = { info: { role, name: "s-x", plan: role === "manager" ? null : "p1", planLead: role === "lead" }, facts: { ownPrompt: <T>(show: () => Promise<T>) => show() } } as unknown as Run;
  host.ctx = (ui ? f.pi.ctx() : { ...f.pi.ctx(), ui: undefined }) as Host["ctx"];
  registerExternalApproveTool(host);
  const tool = f.pi.tools.find((x) => x.name === "cpd_external_approve");
  const prCalls = () => f.argvs().filter((a) => a[0] === "pr");
  return { f, host, tool, prCalls };
}
const need = <T>(x: T | undefined): T => {
  assert.ok(x, "the tool cpd_external_approve is registered");
  return x;
};
const REPLY = { kind: "pr_reply", workflow: "wf-1", to: "item-42", message: "Thanks, fixed in abc123.", reason: "answer the reviewer" };
const POST = { kind: "pr_post", workflow: "wf-1", reason: "post the summary" };
const SLACK = { kind: "slack_send", channel: "C123", message: "Deploy finished\nall green", thread_ts: "1700.01", reason: "tell the team" };

test("the tool cpd_external_approve registers once per host", (t) => {
  const { f, host } = setup(t, "lead");
  registerExternalApproveTool(host);
  assert.equal(f.pi.tools.filter((x) => x.name === "cpd_external_approve").length, 1);
});

test("pr_reply shows the workflow, the item and the text and runs cpd pr approve --kind reply on Allow once", async (t) => {
  const { f, tool, prCalls } = setup(t, "lead");
  const running = f.pi.runTool(need(tool), REPLY);
  const d = f.pi.openDialog("select");
  assert.ok(d, "the dialog is open");
  assert.ok(d.title.includes("wf-1") && d.title.includes("item-42") && d.title.includes(REPLY.message), d.title);
  assert.deepEqual(d.options, ["Allow once", "Deny"]);
  d.answer("Allow once");
  await running;
  assert.deepEqual(prCalls(), [["pr", "approve", "--kind", "reply", "--workflow", "wf-1", "--to", "item-42"]]);
});

test("pr_reply refuses an empty to before any dialog", async (t) => {
  const { f, tool, prCalls } = setup(t, "lead");
  await assert.rejects(f.pi.runTool(need(tool), { ...REPLY, to: "" }));
  assert.equal(f.pi.dialogs.length, 0);
  assert.deepEqual(prCalls(), []);
});

test("pr_post runs cpd pr approve --kind post on Allow once", async (t) => {
  const { f, tool, prCalls } = setup(t, "lead");
  const running = f.pi.runTool(need(tool), POST);
  const d = f.pi.openDialog("select");
  assert.ok(d, "the dialog is open");
  assert.ok(d.title.includes("wf-1"), d.title);
  d.answer("Allow once");
  await running;
  assert.deepEqual(prCalls(), [["pr", "approve", "--kind", "post", "--workflow", "wf-1"]]);
});

test("Deny runs nothing", async (t) => {
  for (const params of [REPLY, POST, SLACK]) {
    const { f, tool, prCalls } = setup(t, "lead");
    const running = f.pi.runTool(need(tool), params);
    const d = f.pi.openDialog("select");
    assert.ok(d, `${params.kind}: the dialog is open`);
    d.answer("Deny");
    const out = await running;
    assert.ok(out.text.length > 0);
    assert.deepEqual(prCalls(), [], params.kind);
    assert.equal(takeSlackSend("mcp__slack__slack_send_message", { channel_id: "C123", message: SLACK.message, thread_ts: "1700.01" }), false, "no grant");
  }
});

test("slack_send shows the channel and the whole message and records a one-shot grant on Allow once", async (t) => {
  const { f, tool } = setup(t, "manager");
  const running = f.pi.runTool(need(tool), SLACK);
  const d = f.pi.openDialog("select");
  assert.ok(d, "the dialog is open");
  assert.ok(d.title.includes("C123") && d.title.includes("1700.01") && d.title.includes(SLACK.message), d.title);
  d.answer("Allow once");
  await running;
  assert.deepEqual(f.argvs(), [], "no cpd command");
  const input = { channel_id: "C123", message: SLACK.message, thread_ts: "1700.01" };
  assert.equal(takeSlackSend("mcp__slack__slack_send_message_draft", input), false, "default tool is slack_send_message");
  assert.equal(takeSlackSend("mcp__slack__slack_send_message", input), true);
  assert.equal(takeSlackSend("mcp__slack__slack_send_message", input), false, "one-shot");

  const again = f.pi.runTool(need(tool), { ...SLACK, slack_tool: "slack_send_message_draft" });
  f.pi.openDialog("select")?.answer("Allow once");
  await again;
  assert.equal(takeSlackSend("mcp__slack__slack_send_message_draft", input), true, "draft grant");
});

test("the pr kinds are for a Plan Lead only and slack_send for a Lead or a Manager", async (t) => {
  for (const role of ["manager", "worker", undefined]) {
    for (const params of [REPLY, POST]) {
      const { f, tool, prCalls } = setup(t, role);
      await assert.rejects(f.pi.runTool(need(tool), params), (e: any) => e.code === "not_a_lead", `${role} ${params.kind}`);
      assert.equal(f.pi.dialogs.length, 0);
      assert.deepEqual(prCalls(), []);
    }
  }
  for (const role of ["worker", undefined]) {
    const { f, tool } = setup(t, role);
    await assert.rejects(f.pi.runTool(need(tool), SLACK), `${role} slack`);
    assert.equal(f.pi.dialogs.length, 0);
  }
  for (const role of ["lead", "manager"]) {
    const { f, tool } = setup(t, role);
    const running = f.pi.runTool(need(tool), SLACK);
    const d = f.pi.openDialog("select");
    assert.ok(d, `${role} slack: the dialog is open`);
    d.answer("Deny");
    await running;
  }
});

test("a dialog needs a UI", async (t) => {
  for (const params of [REPLY, POST, SLACK]) {
    const { f, tool, prCalls } = setup(t, "lead", false);
    await assert.rejects(f.pi.runTool(need(tool), params), (e: any) => e.code === "no_ui", params.kind);
    assert.deepEqual(prCalls(), []);
    assert.equal(f.pi.dialogs.length, 0);
  }
});
