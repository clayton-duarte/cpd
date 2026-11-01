import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { BOUND_SECONDS, blockingReason, reasonText as noblockReason } from "../../lib/noblock.ts";
import { tooLong, unseen, unseenReason } from "../../lib/askguard.ts";
import { reasonText as hostReason } from "../../lib/hostapprove.ts";
import { reasonText as mainReason } from "../../lib/mainguard.ts";
import { reasonText as secretReason } from "../../lib/secretguard.ts";
import { reasonText as testReason, unfilteredTestRun } from "../../lib/testguard.ts";
import { socketPath } from "../../durable-host/config.ts";
import { hostVerdict, type Where } from "../../durable-host/guards.ts";
import { HostRunning } from "../../durable-host/sockets.ts";
import { CONFIG, withHost } from "./rig.ts";

const SOCK = socketPath(CONFIG.home, CONFIG.session);
const running = () => new HostRunning(SOCK);

test("runHost calls the guard with the socket path before any other call", async () => {
  await withHost({}, async (r) => {
    await r.ready();
    assert.equal(r.guards.length, 1, "the host never called the guard");
    assert.equal(r.guards[0]!.path, SOCK);
    assert.equal(r.guards[0]!.calls, 0, "a cpd call came before the guard");
    assert.equal(r.guards[0]!.facts, 0, "an effect came before the guard");
  });
});

test("runHost returns 1 and logs host_running when the guard rejects with HostRunning", async () => {
  await withHost({ guard: running() }, async (r) => {
    await r.ready();
    assert.equal(r.ending.code, 1);
    assert.ok(r.logs.some((l) => l.includes("host_running")), `no host_running in ${JSON.stringify(r.logs)}`);
  });
});

test("runHost makes no cpd call and opens no store when the guard rejects with HostRunning", async () => {
  await withHost({ guard: running() }, async (r) => {
    await r.ready();
    assert.equal(r.ending.code, 1, "the host did not end with 1");
    assert.deepEqual(r.calls, []);
    assert.equal(r.opened.n, 0);
  });
});

test("runHost reports no state when the guard rejects with HostRunning, even when the Session shows stopped", async () => {
  await withHost({ guard: running(), sessionOver: { state: "stopped" } }, async (r) => {
    await r.ready();
    assert.equal(r.ending.code, 1, "the host did not end with 1");
    assert.deepEqual(r.stateCalls(), []);
    assert.deepEqual(r.order.filter((f) => f.startsWith("state:")), []);
  });
});

test("runHost throws a guard failure that is not HostRunning before any effect", async () => {
  const failure = new Error("EACCES: run/host");
  await withHost({ guard: failure }, async (r) => {
    await r.ready();
    assert.equal(r.ending.error, failure);
    assert.deepEqual(r.calls, []);
    assert.equal(r.opened.n, 0);
  });
});

// The Lead guests of contract §6.16: `hostVerdict` over literal calls and a fake `Where`; no git, no socket.

/** A primary checkout (a `.git` directory), a plain directory and a visible chat; no git runs. */
function verdict(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "durable-host-guard-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  const where: Where = { cwd: plain, home: tmp, visible: [] };
  const bash = (arguments_: Record<string, unknown>) => ({ id: "c1", name: "bash", arguments: arguments_ });
  const ask = (arguments_: Record<string, unknown>) => ({ id: "c2", name: "ask_user_question", arguments: arguments_ });
  return { tmp, primary, plain, where, bash, ask };
}

test("hostVerdict refuses a bash sleep over 30 seconds with the lead noblock reason", (t) => {
  const x = verdict(t);
  assert.equal(blockingReason("sleep 300"), "sleep 300", "the lib's ground truth");
  assert.deepEqual(hostVerdict(x.bash({ command: "sleep 300" }), x.where, "lead"), { block: noblockReason("sleep 300", "lead") });
  // an enclosing while whose body sleeps is refused with the loop reason
  assert.deepEqual(hostVerdict(x.bash({ command: "while true; do sleep 300; done" }), x.where, "lead"), { block: noblockReason("while … sleep", "lead") });
});

test("hostVerdict uses the manager noblock reason tail for the same sleep", (t) => {
  const x = verdict(t);
  const manager = hostVerdict(x.bash({ command: "sleep 300" }), x.where, "manager");
  assert.deepEqual(manager, { block: noblockReason("sleep 300", "manager") });
  // the tail is the only role-dependent piece: the two reasons differ by it
  assert.notDeepEqual(manager, { block: noblockReason("sleep 300", "lead") });
  assert.match(noblockReason("sleep 300", "manager"), /is the owning Lead's or Worker's job/);
  assert.match(noblockReason("sleep 300", "lead"), /is left to the engine's Events/);
});

test("hostVerdict refuses watch tail -f cpd events follow and gh run watch with the noblock texts", (t) => {
  const x = verdict(t);
  const cases: readonly [string, string][] = [
    ["watch -n 5 git status", "watch"],
    ["tail -f /var/log/syslog", "tail -f"],
    ["cpd events follow", "cpd events follow"],
    ["gh run watch", "gh run watch"],
  ];
  for (const [command, matched] of cases) {
    assert.equal(blockingReason(command), matched, command);
    assert.deepEqual(hostVerdict(x.bash({ command }), x.where, "lead"), { block: noblockReason(matched, "lead") }, command);
  }
});

test("hostVerdict rewrites a bash without a timeout to one with a 30 second timeout", (t) => {
  const x = verdict(t);
  assert.equal(BOUND_SECONDS, 30);
  assert.deepEqual(hostVerdict(x.bash({ command: "ls -la" }), x.where, "lead"), { arguments: { command: "ls -la", timeout: BOUND_SECONDS } });
  assert.deepEqual(hostVerdict(x.bash({ command: "git status", extra: true }), x.where, "manager"), { arguments: { command: "git status", extra: true, timeout: BOUND_SECONDS } });
});

test("hostVerdict leaves a sleep under 30 seconds with a timeout through", (t) => {
  const x = verdict(t);
  assert.deepEqual(hostVerdict(x.bash({ command: "sleep 10", timeout: 10 }), x.where, "lead"), undefined);
  assert.deepEqual(hostVerdict(x.bash({ command: "sleep 10", timeout: BOUND_SECONDS }), x.where, "manager"), undefined);
  // over the bound the call is already refused as an explicit bash timeout
  assert.deepEqual(hostVerdict(x.bash({ command: "sleep 10", timeout: 120 }), x.where, "lead"), { block: noblockReason("bash timeout 120", "lead") });
});

test("hostVerdict blocks an ask_user_question over 1500 characters with the too-long text", (t) => {
  const x = verdict(t);
  const args = { questions: [{ question: "?", header: "H", options: [{ label: "yes", description: "x".repeat(1500) }] }] };
  const expected = tooLong(args);
  assert.ok(expected, "the lib's ground truth says the call is too long");
  assert.deepEqual(hostVerdict(x.ask(args), x.where, "lead"), { block: expected });
  assert.equal(unseen(args, "c2", x.where.visible), undefined, "the too-long reason comes first");
});

test("hostVerdict blocks an ask_user_question citing a label unseen in the call or the visible chat", (t) => {
  const x = verdict(t);
  // B2-B4 cites B3, which is stated neither in the call's own text nor in the visible chat
  const args = { questions: [{ question: "Proceed with B2-B4?", options: [{ label: "yes" }] }] };
  const expected = unseen(args, "c2", x.where.visible);
  assert.equal(expected, unseenReason(["B3"]));
  assert.deepEqual(hostVerdict(x.ask(args), x.where, "manager"), { block: expected });
  // a visible chat entry that states B3 clears it
  const visible = [{ type: "custom_message", display: true, content: "B3 is decided" }];
  assert.equal(unseen(args, "c2", visible), undefined);
  assert.deepEqual(hostVerdict(x.ask(args), { ...x.where, visible }, "lead"), undefined);
});

test("hostVerdict lets an ask_user_question citing its own labels through", (t) => {
  const x = verdict(t);
  const args = { questions: [{ question: "About A1 and B2?", options: [{ label: "yes", description: "B2 stands" }] }] };
  assert.equal(unseen(args, "c2", x.where.visible), undefined);
  assert.deepEqual(hostVerdict(x.ask(args), x.where, "lead"), undefined);
});

test("hostVerdict never applies the test guard for a lead or manager", (t) => {
  const x = verdict(t);
  for (const role of ["lead", "manager"] as const) {
    for (const command of ["cargo test --workspace", "cargo test", "npm test"]) {
      const matched = unfilteredTestRun(command);
      assert.ok(matched, `${command} is an unfiltered run`);
      const v = hostVerdict(x.bash({ command }), x.where, role);
      assert.notDeepEqual(v, { block: testReason(matched) }, `${role}: ${command} must not take the worker's test guard`);
      assert.deepEqual(v, { arguments: { command, timeout: BOUND_SECONDS } }, `${role}: ${command}`);
    }
  }
});

test("hostVerdict keeps blocking primary checkout writes and raw secret and host approvals", (t) => {
  const x = verdict(t);
  const abs = join(x.primary, "docs", "commands.md");
  for (const role of ["lead", "manager"] as const) {
    assert.deepEqual(hostVerdict({ id: "c", name: "edit", arguments: { path: abs } }, x.where, role), { block: mainReason(`edit ${abs} in ${x.primary}`) }, role);
    assert.deepEqual(hostVerdict({ id: "c", name: "write", arguments: { path: abs } }, x.where, role), { block: mainReason(`write ${abs} in ${x.primary}`) }, role);
    const f = join(x.primary, "f");
    assert.deepEqual(hostVerdict(x.bash({ command: `echo x > ${f}` }), x.where, role), { block: mainReason(`> ${f} in ${x.primary}`) }, role);
    assert.deepEqual(hostVerdict(x.bash({ command: "cpd secret approve --action get --vault v --item i --field f" }), x.where, role), { block: secretReason("cpd secret approve") }, role);
    assert.deepEqual(hostVerdict(x.bash({ command: "cpd host approve --reason dirty" }), x.where, role), { block: hostReason("cpd host approve") }, role);
  }
});
