import assert from "node:assert/strict";
import { test } from "node:test";
import { reasonText, secretBypass } from "../lib/secretguard.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.17: no agent calls 1Password's op, and no agent writes an approval.

test("secretBypass refuses op as a command word, after prefixes and as a path", () => {
  const cases: [string, string][] = [
    ["op read op://cpd/x/y", "op read"],
    ["op item get x", "op item"],
    ["op", "op"],
    ["FOO=1 op read x", "op read"],
    ["sudo env op read x", "op read"],
    ["/opt/homebrew/bin/op whoami", "op whoami"],
    ["time op read x", "op read"],
  ];
  for (const [command, matched] of cases) assert.equal(secretBypass(command), matched, command);
});

test("secretBypass passes op as an argument, in quotes and inside another word", () => {
  for (const c of ["echo op read", 'grep "op read" f', "cpd secret get x --vault cpd --field f", "ls /opt/open", "tool --op", "desktop read", "stop read"]) {
    assert.equal(secretBypass(c), undefined, c);
  }
  assert.equal(secretBypass('echo "op read"; op read x'), "op read"); // only the second command is judged
});

test("secretBypass refuses cpd secret approve and passes the other cpd secret commands", () => {
  const approve = "cpd secret approve --action get --vault v --item i --field f";
  assert.equal(secretBypass(approve), "cpd secret approve");
  assert.equal(secretBypass(`cd x && ${approve}`), "cpd secret approve");
  assert.equal(secretBypass("env FOO=1 cpd secret approve --action list --vault v"), "cpd secret approve");
  for (const s of ["get x --vault cpd --field f", "list --vault cpd", "create x --vault cpd", "edit x --vault cpd", "delete x --vault cpd", "allow x --attempt a --vault cpd --field f"]) {
    assert.equal(secretBypass(`cpd secret ${s}`), undefined, s);
  }
});

test("secretBypass judges every simple command of a list and a pipeline", () => {
  assert.equal(secretBypass("a && op read x; echo y | op item list"), "op read");
  assert.equal(secretBypass("echo y | op item list"), "op item");
  assert.equal(secretBypass("echo a; ls && cpd secret approve --action list --vault v"), "cpd secret approve");
});

test("secretBypass does not judge sh -c, xargs and scripts", () => {
  for (const c of ["sh -c 'op read x'", "xargs op read", "./script.sh", "$(echo op) read x"]) assert.equal(secretBypass(c), undefined, c);
  assert.equal(secretBypass("sh -c 'op read x'; op read y"), "op read"); // the plain command beside it is still judged
});

test("the reason names cpd secret and what was refused", () => {
  assert.equal(
    reasonText("op read"),
    "φ: agents do not call 1Password's op; use cpd secret get|list|create|edit|delete (commands.md §24); refused: op read",
  );
  assert.equal(
    reasonText("cpd secret approve"),
    "φ: approvals are the User's: ask with the tool cpd_secret_approve (commands.md §15.17); refused: cpd secret approve",
  );
  assert.ok(!reasonText("op read").includes("\n"));
});

// ---- through the adapter ----

type Result = { block?: boolean; reason?: string } | undefined;

test("the adapter blocks a bash op call of every role", async (t) => {
  for (const role of ["lead", "manager", "worker"]) {
    const f = new Fixture();
    t.after(() => f.close());
    f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
    f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
    f.load();
    await f.pi.fire("session_start");
    await waitFor("the follower", () => f.follows().length > 0);
    const call = async (command: string) => (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command } }))[0] as Result;
    const r = await call("op read x");
    assert.equal(r?.block, true, role);
    assert.equal(r?.reason, reasonText("op read"), role);
    assert.equal(await call("ls"), undefined, role);
  }
});
