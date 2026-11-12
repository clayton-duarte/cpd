import { Facts } from "../lib/state.ts";
import assert from "node:assert/strict";
import { test, type TestContext } from "node:test";
import { createHost, type Host } from "../lib/host.ts";
import { registerHostApproveTool } from "../lib/hostapprove.ts";
import type { Run } from "../lib/run.ts";
import { Fixture, waitFor as waitForDialog } from "./fake.ts";

// commands.md §15.18 "The pi tool cpd_host_approve", driven through the fake host.
// The exact dialog sentence is the doc's: the tool provides it because pi gives the dialog no title.

const NAME = "host-manager";
const REASON = "dirty";
const TITLE = `${NAME} asks to repair the cpd checkout. Reason: ${REASON}. Allow once: approve the repair. It discards tracked changes of ~/projects/cpd and resets main to origin/main after saving the diff and the old HEAD.`;

/** A host with an active run of `role` (undefined: no run) and the fake's context, with the host-approve tool registered. */
function setup(t: TestContext, role: string | undefined, ui = true) {
  const f = new Fixture();
  t.after(() => f.close());
  const host: Host = createHost(f.pi, { env: process.env });
  if (role !== undefined) host.run = { info: { role, name: NAME, plan: role === "manager" ? null : "p1", planLead: role === "lead" }, facts: new Facts() } as unknown as Run;
  host.ctx = (ui ? f.pi.ctx() : { ...f.pi.ctx(), ui: undefined }) as Host["ctx"];
  registerHostApproveTool(host);
  const tool = f.pi.tools.find((x) => x.name === "cpd_host_approve");
  assert.ok(tool, "the tool cpd_host_approve is registered");
  const hostCalls = () => f.argvs().filter((a) => a[0] === "host");
  return { f, tool, hostCalls };
}

test("the tool cpd_host_approve is for Manager Sessions only", async (t) => {
  for (const role of ["lead", "worker", undefined]) {
    const { f, tool, hostCalls } = setup(t, role);
    await assert.rejects(
      f.pi.runTool(tool, { reason: REASON }),
      (e: any) => e.code === "not_a_manager" && e.message === "the tool cpd_host_approve is for Manager Sessions only",
      String(role),
    );
    assert.equal(f.pi.dialogs.length, 0, String(role));
    assert.deepEqual(hostCalls(), [], String(role));
  }
});

test("Allow once runs cpd host approve and tells the Manager to run cpd host repair now", async (t) => {
  const { f, tool, hostCalls } = setup(t, "manager");
  const running = f.pi.runTool(tool, { reason: REASON });
  const d = await waitForDialog("approval dialog", () => f.pi.openDialog("select"));
  assert.ok(d, "the dialog is open");
  assert.equal(d.title, TITLE);
  assert.deepEqual(d.options, ["Allow once", "Deny"]);
  d.answer("Allow once");
  const out = await running;
  assert.deepEqual(hostCalls(), [["host", "approve"]]);
  assert.equal(out.text, "Approved: run cpd host repair now.");
});

test("Deny runs nothing and tells the Manager not to retry", async (t) => {
  const { f, tool, hostCalls } = setup(t, "manager");
  const running = f.pi.runTool(tool, { reason: REASON });
  const d = await waitForDialog("approval dialog", () => f.pi.openDialog("select"));
  assert.ok(d, "the dialog is open");
  d.answer("Deny");
  const out = await running;
  assert.equal(out.text, "The User denied it. Do not try again.");
  assert.deepEqual(hostCalls(), []);
});
