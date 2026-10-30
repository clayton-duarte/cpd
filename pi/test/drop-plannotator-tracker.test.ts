import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";
import { Fixture, planShow, shown } from "./fake.ts";

// commands.md §15 with the plannotator tracker and guard removed (hs-d738, hs-d739).

const reviewCalls = (f: Fixture) => f.argvs().filter((a) => a[0] === "review");

async function started(f: Fixture, manager: boolean) {
  f.respond("plan-show", planShow());
  if (manager) f.respond("session-show", shown({ role: "manager", plan: null, plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  await f.quiet();
}

test("drop-plannotator startup runs no cpd review command", async (t) => {
  for (const manager of [false, true]) {
    const f = new Fixture();
    t.after(() => f.close());
    await started(f, manager);
    assert.deepEqual(reviewCalls(f), [], manager ? "a Manager" : "a Lead");
  }
});

test("drop-plannotator a plannotator tool call is neither blocked nor tracked", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, false);
  const results = await f.pi.fireWith("tool_call", { toolCallId: "pn1", toolName: "plannotator", input: { action: "annotate", target: "a.md" } });
  assert.deepEqual(results.filter((r) => r !== undefined && (r as { block?: boolean }).block), [], "not blocked");
  await f.pi.fire("tool_result", { toolCallId: "pn1", toolName: "plannotator", input: { action: "annotate", target: "a.md" }, content: [], isError: false });
  await f.quiet();
  assert.deepEqual(reviewCalls(f), [], "no cpd review open or close");
});

test("drop-plannotator the tracker and guard modules are gone", () => {
  for (const m of ["pnguard.ts", "pntrack.ts"]) assert.equal(existsSync(new URL(`../lib/${m}`, import.meta.url)), false, m);
});
