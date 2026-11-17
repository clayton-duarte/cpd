import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Fixture, shown } from "./fake.ts";

// Decision pro-d56, commands.md §15.20: a Lead runs a command the User explicitly asks for.

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const lead = read("../skills/cpd-lead/SKILL.md");
test("Lead responds to explicitly requested commands and owns process cleanup (pro-d56)", () => {
  assert.match(lead, /User explicitly requests a command, run it within applicable authority and controls/);
  assert.match(lead, /stopping and cleaning up any long-running process you start/);
});

test("a Lead's background start and stop pass the adapter's guards and the guards still refuse what stays (pro-d56)", async (t) => {
  const start = "set -m; cd /tmp/cpd-p1-wt && npm run dev > /tmp/cpd-p1/dev.log 2>&1 & echo $! > /tmp/cpd-p1/dev.pid";
  const stop = "kill -TERM -- -$(cat /tmp/cpd-p1/dev.pid)";

  const primary = mkdtempSync(join(tmpdir(), "uc-primary-"));
  t.after(() => rmSync(primary, { recursive: true, force: true }));
  mkdirSync(join(primary, ".git"));

  const f = new Fixture();
  f.respond("session-show", shown({ role: "lead", plan: "p1", plan_lead: true }));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  t.after(() => f.close());

  const call = async (command: string) => (await f.pi.fireWith("tool_call", { toolName: "bash", toolCallId: "t", input: { command } }))[0] as { block?: boolean } | undefined;
  assert.equal(await call(start), undefined, "the START line must run");
  assert.equal(await call(stop), undefined, "the STOP line must run");
  for (const c of ["op item get x", "sleep 60", "timeout 120 npm run dev", `git -C ${primary} checkout main`])
    assert.equal((await call(c))?.block, true, `${c} must stay refused`);
});
