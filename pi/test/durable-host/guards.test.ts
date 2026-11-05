import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { tooLong, unseen, unseenReason } from "../../lib/askguard.ts";
import { reasonText as hostReason } from "../../lib/hostapprove.ts";
import { reasonText as mainReason } from "../../lib/mainguard.ts";
import { reasonText as secretReason } from "../../lib/secretguard.ts";
import { leadExploreGuard } from "../../lib/explore.ts";
import { hostVerdict, type Where } from "../../durable-host/guards.ts";
import type { Host } from "../../lib/host.ts";

/** A primary checkout (a `.git` directory), a plain directory and the visible chat the G4 guard reads; no git runs. */
function fixtures(t: TestContext) {
  const tmp = mkdtempSync(join(tmpdir(), "durable-host-guards-"));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const primary = join(tmp, "projects", "repo");
  mkdirSync(join(primary, ".git"), { recursive: true });
  mkdirSync(join(primary, "docs"));
  const plain = join(tmp, "plain");
  mkdirSync(plain);
  const where: Where = { cwd: plain, home: tmp, visible: [] };
  return { tmp, primary, plain, where };
}

test("hostVerdict refuses an edit inside a primary checkout", (t) => {
  const x = fixtures(t);
  const abs = join(x.primary, "docs", "commands.md");
  for (const name of ["edit", "write"]) {
    assert.deepEqual(hostVerdict({ id: "c", name, arguments: { path: abs } }, x.where, "lead"), { block: mainReason(`${name} ${abs} in ${x.primary}`) }, name);
  }
  const f = join(x.primary, "f");
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: `echo x > ${f}` } }, x.where, "lead"), { block: mainReason(`> ${f} in ${x.primary}`) });
  assert.equal(hostVerdict({ id: "c", name: "write", arguments: { path: join(x.plain, "f") } }, x.where, "lead"), undefined);
});

test("hostVerdict refuses raw cpd secret approve and raw cpd host approve in bash", (t) => {
  const x = fixtures(t);
  assert.deepEqual(
    hostVerdict({ id: "c", name: "bash", arguments: { command: "cpd secret approve --action get --vault v --item i --field f" } }, x.where, "lead"),
    { block: secretReason("cpd secret approve") },
  );
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: "cpd host approve --reason dirty" } }, x.where, "lead"), { block: hostReason("cpd host approve") });
});

test("hostVerdict does not apply the unfiltered test guard", (t) => {
  const x = fixtures(t);
  for (const role of ["lead", "manager"] as const) {
    for (const command of ["cargo test --workspace", "cargo test", "npm test"]) {
      // no test-guard block: a Lead's or Manager's plain bash is only rewritten to carry the 30 s timeout (noblock)
      assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command } }, x.where, role), { arguments: { command, timeout: 30 } }, `${role}: ${command}`);
    }
  }
});

test("hostVerdict does not apply the explore profile", (t) => {
  const x = fixtures(t);
  // an explore Worker could not write outside its artefact; a Lead may write anywhere outside a primary checkout.
  assert.equal(hostVerdict({ id: "c", name: "write", arguments: { path: join(x.plain, "notes.md") } }, x.where, "lead"), undefined);
  // a bash with no timeout is rewritten to carry the 30 s timeout (noblock), not blocked by an explore profile
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: "echo x > notes.md" } }, x.where, "lead"), { arguments: { command: "echo x > notes.md", timeout: 30 } });
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: "ls -la" } }, x.where, "lead"), { arguments: { command: "ls -la", timeout: 30 } });
});

// The ask guards of contract §6.16 pinned outside the named tests too (tooLong, unseen).
test("hostVerdict refuses a too-long ask_user_question with the lib's text", (t) => {
  const x = fixtures(t);
  const args = { questions: [{ question: "x".repeat(1501) }] };
  assert.deepEqual(hostVerdict({ id: "c", name: "ask_user_question", arguments: args }, x.where, "lead"), { block: tooLong(args) });
});

test("hostVerdict refuses an ask_user_question whose range cites an unseen label", (t) => {
  const x = fixtures(t);
  const args = { questions: [{ question: "B2-B4?" }] };
  assert.equal(unseen(args, "c", x.where.visible), unseenReason(["B3"]));
  assert.deepEqual(hostVerdict({ id: "c", name: "ask_user_question", arguments: args }, x.where, "manager"), { block: unseenReason(["B3"]) });
});

// The per-turn bounds (contract §6.16 "The per-turn bounds"): the lib's toolCallGuard runs first in hostVerdict over `where.loop`.
const READ_STREAK_REASON =
  "φ: loop guard: this tool call was NOT executed: more than 12 consecutive read-only tool calls in one turn. Stop your turn now. Delegate codebase exploration to a scout Job (cpd job add <k> --kind scout), or run the cpd state command that records your decision.";
const TOTAL_CALLS_REASON =
  "φ: loop guard: this tool call was NOT executed: more than 30 read-only tool calls in one turn. Stop your turn now: end it with what you have, and delegate any further codebase exploration to a scout Job.";

/** A fake loop host: the run the lib reads, a fixed clock and a fake entry writer; nothing real runs. */
function loopWhere(x: { where: Where }) {
  const run = {
    role: "lead",
    info: { role: "lead", loopGuardTurns: 20 },
    turnStarts: [] as number[],
    tripped: false,
    readStreak: 0,
    totalCalls: 0,
    turnRefusals: 0,
    facts: { open: () => {}, close: () => {} },
  };
  const host = { run, options: { now: () => 1_000_000 }, pi: { appendEntry: () => {} }, refresh: () => {} } as unknown as Host;
  return { where: { ...x.where, loop: host } as Where, run };
}
let n = 0;
const read = (where: Where, role: "lead" | "manager" = "lead") => hostVerdict({ id: `c${++n}`, name: "read", arguments: { path: "/x" } }, where, role);

test("hostVerdict refuses the 13th consecutive read-only call of a turn", (t) => {
  const { where } = loopWhere(fixtures(t));
  for (let i = 1; i <= 12; i++) assert.equal(read(where), undefined, `read ${i} runs`);
  assert.deepEqual(read(where), { block: READ_STREAK_REASON });
});

test("hostVerdict refuses the 31st tool call of a turn", (t) => {
  const { where } = loopWhere(fixtures(t));
  // a state write resets the streak, not the total: ten reads, a write, three times, make 30 reads
  for (let round = 0; round < 3; round++) {
    for (let i = 0; i < 10; i++) assert.equal(read(where), undefined, `round ${round} read ${i}`);
    assert.equal(hostVerdict({ id: "w", name: "cpd_plan_review", arguments: {} }, where, "lead"), undefined);
  }
  assert.deepEqual(read(where), { block: TOTAL_CALLS_REASON });
});

test("a state write MCP mutation or codemode call resets the read streak", (t) => {
  const writes: { name: string; arguments: Record<string, unknown> }[] = [
    { name: "cpd_plan_review", arguments: {} },
    { name: "bash", arguments: { command: "cpd job add k --kind scout", timeout: 5 } },
    { name: "mcp__atlassian__createJiraIssue", arguments: {} },
    { name: "codemode", arguments: { code: "await tools.mcp__slack__slack_send_message({})" } },
  ];
  for (const w of writes) {
    const { where } = loopWhere(fixtures(t));
    for (let i = 1; i <= 12; i++) assert.equal(read(where), undefined, `${w.name}: read ${i} before`);
    assert.equal(hostVerdict({ id: "w", ...w }, where, "lead"), undefined, `${w.name} runs`);
    for (let i = 1; i <= 12; i++) assert.equal(read(where), undefined, `${w.name}: read ${i} after`);
    assert.deepEqual(read(where), { block: READ_STREAK_REASON }, `${w.name}: the streak counts again from 0`);
  }
});

test("hostVerdict refuses a Lead read of an explore artefact", (t) => {
  const x = fixtures(t);
  const artefact = join(x.tmp, ".cpd", "run", "p1", "explore", "f1.md");
  const refusal = leadExploreGuard("lead", { CPD_HOME: join(x.tmp, ".cpd") }, { name: "read", arguments: { path: artefact } });
  assert.ok(refusal !== undefined && "block" in refusal, "the lib refuses it");
  assert.deepEqual(hostVerdict({ id: "c", name: "read", arguments: { path: artefact } }, x.where, "lead"), refusal);
  assert.deepEqual(hostVerdict({ id: "c", name: "bash", arguments: { command: `cat ${artefact}`, timeout: 5 } }, x.where, "lead"), refusal);
  assert.equal(hostVerdict({ id: "c", name: "read", arguments: { path: join(x.plain, "notes.md") } }, x.where, "lead"), undefined);
});
