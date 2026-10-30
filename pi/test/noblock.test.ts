import assert from "node:assert/strict";
import { test } from "node:test";
import { BOUND_SECONDS, blockingReason, reasonText } from "../lib/noblock.ts";
import { Fixture, shown, waitFor } from "./fake.ts";

// commands.md §15.11: a Manager's or a Lead's bash call that waits over 30 s is refused.

const refused = (command: string, timeout?: number) => assert.notEqual(blockingReason(command, timeout), undefined, `${command} (timeout ${timeout}) must be refused`);
const passes = (command: string, timeout?: number) => assert.equal(blockingReason(command, timeout), undefined, `${command} (timeout ${timeout}) must run`);

test("sleep: 30 s passes, over it is refused, in any unit", () => {
  assert.equal(BOUND_SECONDS, 30);
  passes("sleep 30");
  passes("sleep 30s");
  passes("sleep 0.5m");
  passes("sleep 5");
  passes("sleep $n");
  refused("sleep 31");
  refused("sleep 30.5");
  refused("sleep 1m");
  refused("sleep 0.6m");
  refused("sleep 1h");
  refused("sleep 1d");
  refused("sleep infinity");
  refused("sleep 20 20");
  refused("/bin/sleep 60");
  refused("echo start; sleep 31 && echo done");
});

test("timeout: 20 passes, 60 is refused, its options are skipped", () => {
  passes("timeout 20 cmd");
  passes("timeout 30 cmd");
  refused("timeout 60 cmd");
  refused("timeout 2m cmd");
  refused("timeout -s KILL 60 cmd");
  refused("timeout --preserve-status -k 5 60 cmd");
  passes("timeout -s KILL 20 cmd");
});

test("watch, tail -f, cpd events follow, gh run watch, gh pr checks --watch", () => {
  refused("watch -n 1 date");
  refused("tail -f log");
  refused("tail -F log");
  refused("tail -n 5 -f log");
  refused("tail -fn5 log");
  refused("tail --follow=name log");
  passes("tail -n 20 log");
  refused("cpd events follow --session x");
  passes("cpd events list --context c");
  refused("gh run watch 123");
  refused("gh pr checks 12 --watch");
  passes("gh pr checks 12");
  passes("gh run list");
});

test("a while or until loop whose body sleeps is refused, whatever the sleep", () => {
  refused("while true; do cpd plan show; sleep 5; done");
  refused("until cpd plan show; do sleep 1; done");
  refused("while sleep 1; do x; done");
  refused("while true\ndo\n  sleep 2\ndone");
  passes("while read l; do echo $l; done < f");
  passes("for i in 1 2; do sleep 5; done");
});

test("the bash tool's own timeout above 30 s is refused", () => {
  refused("ls", 120);
  refused("ls", 31);
  passes("ls", 30);
  passes("ls", 10);
  passes("ls");
});

test("commands that match nothing run, and a pattern in an argument or in quotes is not a command", () => {
  passes("cpd plan show");
  passes("git log --grep watch");
  passes("grep -r 'sleep 60' .");
  passes('echo "watch; sleep 100"');
  passes("echo sleep 100");
  passes("# sleep 100");
  passes("cpd --timeout 60 plan show");
  passes("");
});

test("the reason is one line naming the rule and what matched", () => {
  const r = reasonText(blockingReason("sleep 1m")!, "manager");
  assert.equal(r, "φ: a Manager never blocks: a wait over 30 s runs in the background or is the owning Lead's or Worker's job; refused: sleep 1m");
  assert.equal(r.includes("\n"), false);
  assert.match(reasonText(blockingReason("ls", 120)!, "manager"), /refused: bash timeout 120$/);
});

test("the Lead reason names a Lead and leaves the wait to the background or the engine Events", () => {
  const r = reasonText(blockingReason("sleep 1m")!, "lead");
  assert.equal(r, `φ: a Lead never blocks: a wait over 30 s runs in the background or is left to the engine's Events; refused: sleep 1m`);
  assert.equal(r.includes("\n"), false);
  assert.match(reasonText(blockingReason("ls", 120)!, "lead"), /refused: bash timeout 120$/);
});

test("a heredoc body is not judged: prose, backticked words and waits inside it pass", () => {
  passes("cat << EOF\nrun `watch` to follow\nsleep 60\nEOF");
  passes("cat <<'EOF'\nrun `watch` to follow\nsleep 60\nEOF");
  passes("cat <<\"EOF\"\nrun `watch` to follow\nsleep 60\nEOF");
  passes("cat <<\\EOF\nrun `watch` to follow\nsleep 60\nEOF");
  passes("cat > /tmp/x <<EOF\nwhile true; do sleep 5; done\nEOF");
  passes("cat <<-EOF\n\twatch\n\tEOF");
  passes("cat <<EOF\ndon't watch\nEOF");
  passes("cat <<EOF\ntimeout 60 cmd\nEOF");
});

test("a heredoc ends at its own delimiter line: only an exact line ends it and the command after it is judged", () => {
  assert.equal(blockingReason("cat <<EOF\nhi\nEOF\nwatch"), "watch");
  passes("cat <<EOF\n EOF\nwatch\nEOF");
  passes("cat <<EOF\nEOFX\nwatch\nEOF");
  assert.equal(blockingReason("cat <<-EOF\n\tEOF\nwatch"), "watch");
});

test("an unterminated heredoc runs to the end of the call", () => {
  passes("cat <<EOF\nwatch");
  passes("echo hi; cat <<EOF\nsleep 60");
});

test("two heredocs on one line skip both bodies in order", () => {
  passes("cat <<A <<B\nwatch\nA\nsleep 60\nB\nls");
  assert.equal(blockingReason("cat <<A <<B\nx\nA\ny\nB\nwatch"), "watch");
});

test("the line of a heredoc operator and a here-string are still judged", () => {
  assert.equal(blockingReason("cat <<EOF; sleep 60\nbody\nEOF"), "sleep 60");
  assert.equal(blockingReason("cat <<EOF | watch\nbody\nEOF"), "watch");
  assert.equal(blockingReason('cat <<< "hi"\nwatch'), "watch");
});

// ---- through the adapter ----

async function started(role: string) {
  const f = new Fixture();
  f.respond("session-show", shown({ role, plan: role === "manager" ? null : "p1", plan_lead: role === "lead" }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
  return f;
}

const call = async (f: Fixture, input: Record<string, unknown>, toolName = "bash") => (await f.pi.fireWith("tool_call", { toolName, toolCallId: "t", input }))[0] as { block?: boolean; reason?: string } | undefined;

const CASES: [Record<string, unknown>, boolean][] = [
  [{ command: "sleep 31" }, true],
  [{ command: "sleep 1m" }, true],
  [{ command: "watch -n 2 cpd plan show" }, true],
  [{ command: "while true; do cpd plan show; sleep 5; done" }, true],
  [{ command: "ls", timeout: 120 }, true],
  [{ command: "for i in 1 2; do git fetch origin; sleep 280; done" }, true],
  [{ command: "sleep 30" }, false],
  [{ command: "for i in 1 2; do sleep 5; done" }, false],
  [{ command: "cpd plan show" }, false],
  [{ command: "timeout 20 cmd" }, false],
];

test("a Manager's refused calls get block and the rule's reason; the others run", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  for (const [input, isRefused] of CASES) {
    const r = await call(f, { ...input });
    if (isRefused) {
      assert.equal(r?.block, true, JSON.stringify(input));
      assert.match(r?.reason ?? "", /^φ: a Manager never blocks: .* refused: /);
    } else assert.equal(r, undefined, JSON.stringify(input));
  }
});

test("a Manager's other tools are never refused", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  assert.equal(await call(f, { command: "sleep 60", timeout: 120 }, "read"), undefined);
});

test("a Lead refused calls get block and the Lead reason and the others run", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  for (const [input, isRefused] of CASES) {
    const r = await call(f, { ...input });
    if (isRefused) {
      assert.equal(r?.block, true, JSON.stringify(input));
      assert.match(r?.reason ?? "", /^φ: a Lead never blocks: .* refused: /);
    } else assert.equal(r, undefined, JSON.stringify(input));
  }
});

test("a Worker identical calls are never refused", async (t) => {
  const f = await started("worker");
  t.after(() => f.close());
  for (const [input] of CASES) assert.equal(await call(f, { ...input }), undefined, `worker: ${JSON.stringify(input)}`);
});

test("a Lead other tools are never refused", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  assert.equal(await call(f, { command: "sleep 60", timeout: 120 }, "read"), undefined);
});

test("before session_start no call is refused", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.load();
  assert.equal(await call(f, { command: "sleep 60" }), undefined);
});

test("a Manager's bash call without timeout gets timeout 30 set in place", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  const input: Record<string, unknown> = { command: "cpd plan show" };
  assert.equal(await call(f, input), undefined);
  assert.equal(input.timeout, BOUND_SECONDS);
});

test("a Manager's bash call with timeout 10 keeps timeout 10", async (t) => {
  const f = await started("manager");
  t.after(() => f.close());
  const input: Record<string, unknown> = { command: "cpd plan show", timeout: 10 };
  assert.equal(await call(f, input), undefined);
  assert.equal(input.timeout, 10);
});

test("a Lead's bash call without timeout gets timeout 30 set in place", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  const input: Record<string, unknown> = { command: "cpd plan show" };
  assert.equal(await call(f, input), undefined);
  assert.equal(input.timeout, BOUND_SECONDS);
});

test("a Lead's bash call with timeout 10 keeps timeout 10", async (t) => {
  const f = await started("lead");
  t.after(() => f.close());
  const input: Record<string, unknown> = { command: "cpd plan show", timeout: 10 };
  assert.equal(await call(f, input), undefined);
  assert.equal(input.timeout, 10);
});

test("a Worker's bash call without timeout keeps timeout undefined", async (t) => {
  const f = await started("worker");
  t.after(() => f.close());
  const input: Record<string, unknown> = { command: "cpd plan show" };
  assert.equal(await call(f, input), undefined);
  assert.equal(input.timeout, undefined);
  assert.equal("timeout" in input, false);
});
