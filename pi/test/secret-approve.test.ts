import { Facts } from "../lib/state.ts";
import assert from "node:assert/strict";
import { test, type TestContext } from "node:test";
import { createHost, type Host } from "../lib/host.ts";
import { registerSecretTool } from "../lib/secret.ts";
import type { Run } from "../lib/run.ts";
import { Fixture, waitFor } from "./fake.ts";

// commands.md §15.17 "The tool cpd_secret_approve", driven through the fake host.

const NAME = "lead-x";
const ARGS = { action: "get", vault: "other", item: "db", field: "password", reason: "migrate the db" };
const TITLE = `${NAME} asks to get in vault other: item db, field password. Reason: migrate the db`;

/** A host with an active run of `role` (undefined: no run) and the fake's context, with the secret tool registered. */
function setup(t: TestContext, role: string | undefined, ui = true) {
  const f = new Fixture();
  t.after(() => f.close());
  const host: Host = createHost(f.pi, { env: process.env });
  const call = host.call;
  host.call = (args) => args[0] === "secret" && args[1] === "check" ? Promise.resolve({ ok: true }) : call(args);
  if (role !== undefined) host.run = { info: { role, name: NAME, plan: "p1", planLead: role === "lead" }, facts: new Facts() } as unknown as Run;
  host.ctx = (ui ? f.pi.ctx() : { ...f.pi.ctx(), ui: undefined }) as Host["ctx"];
  registerSecretTool(host);
  const tool = f.pi.tools.find((x) => x.name === "cpd_secret_approve");
  assert.ok(tool, "the tool cpd_secret_approve is registered");
  const secretCalls = () => f.argvs().filter((a) => a[0] === "secret");
  return { f, tool, secretCalls, host };
}

test("cpd_secret_approve is refused for a Worker and for no run", async (t) => {
  for (const role of ["worker", undefined]) {
    const { f, tool, secretCalls } = setup(t, role);
    await assert.rejects(f.pi.runTool(tool, ARGS), (e: any) => e.code === "not_a_lead" && e.message === "the tool cpd_secret_approve is for Lead and Manager Sessions only", String(role));
    assert.equal(f.pi.dialogs.length, 0);
    assert.deepEqual(secretCalls(), []);
  }
});

test("cpd_secret_approve shows the User the action, vault, item and field", async (t) => {
  for (const role of ["lead", "manager"]) {
    const { f, tool, host } = setup(t, role);
    const running = f.pi.runTool(tool, ARGS);
    const d = await waitFor("the dialog", () => f.pi.openDialog("select"));
    assert.equal(host.run!.facts.wanted().state, "blocked");
    assert.equal(d.title, TITLE, role);
    assert.deepEqual(d.options, ["Allow once", "Deny"], role);
    d.answer("Deny");
    await running;
    assert.notEqual(host.run!.facts.wanted().state, "blocked");
  }
});

test("Allow once runs cpd secret approve with the same arguments", async (t) => {
  for (const role of ["lead", "manager"]) {
    const { f, tool, secretCalls } = setup(t, role);
    const running = f.pi.runTool(tool, ARGS);
    (await waitFor("the dialog", () => f.pi.openDialog("select"))).answer("Allow once");
    const out = await running;
    assert.deepEqual(secretCalls(), [["secret", "approve", "--action", "get", "--vault", "other", "--item", "db", "--field", "password"]], role);
    assert.equal(out.text, "Approved for one command in the next 10 minutes: run it now.", role);
  }
});

test("Deny runs nothing and tells the agent not to retry", async (t) => {
  const { f, tool, secretCalls } = setup(t, "lead");
  const running = f.pi.runTool(tool, ARGS);
  (await waitFor("the dialog", () => f.pi.openDialog("select"))).answer("Deny");
  const out = await running;
  assert.equal(out.text, "The User denied it. Do not try again.");
  assert.deepEqual(secretCalls(), []);
});

test("cpd_secret_approve refuses cpd and homeserver", async (t) => {
  for (const vault of ["cpd", "homeserver"]) {
    const { f, tool, secretCalls } = setup(t, "lead");
    await assert.rejects(
      f.pi.runTool(tool, { ...ARGS, vault }),
      (e: any) => String(e.message).includes("these vaults need no approval; use cpd secret get|list|create|edit|delete (commands.md §24)"),
      vault,
    );
    assert.equal(f.pi.dialogs.length, 0, vault);
    assert.deepEqual(secretCalls(), [], vault);
  }
});

test("cpd_secret_approve with no dialog is an error", async (t) => {
  const { f, tool, secretCalls } = setup(t, "lead", false);
  await assert.rejects(f.pi.runTool(tool, ARGS), (e: any) => e.code === "no_ui");
  assert.deepEqual(secretCalls(), []);
});

 test("cpd_secret_approve fails fast when op is not signed in", async (t) => {
  const { f, tool, host, secretCalls } = setup(t, "lead");
  host.call = async () => { throw Object.assign(new Error("sign in with op signin"), { code: "secret_not_signed_in" }); };
  const running = f.pi.runTool(tool, ARGS);
  // Give the async tool a turn, then release any incorrectly opened prompt before asserting.
  await Promise.resolve();
  const dialog = f.pi.openDialog("select");
  if (dialog) dialog.answer("Deny");
  const outcome = await running.then(() => null, (e: unknown) => e);
  assert.equal(dialog, undefined, "no approval prompt should open without a signed-in op");
  assert.equal((outcome as { code?: string } | null)?.code, "secret_not_signed_in");
  assert.deepEqual(secretCalls(), []);
});
