import { test } from "node:test";
import assert from "node:assert/strict";
import { startAgent, type StartAgent } from "../../src/herdr/agents.ts";
import { CpdError } from "../../src/lib/errors.ts";
import { says, useFakeHerdr } from "./setup.ts";

/*
 * herdr 0.9.3 transcript (herdr --session cpd-test agent start x --kind amp --pane <pane> --timeout N):
 *   0, 1, 10, 100, 500, 999, 1000, 1001, 3000, 300001 -> invalid_agent_timeout
 *     "agent start timeout must be greater than 3000ms and at most 300000ms"
 *   3001 -> passed validation (then timed out waiting for the absent agent)
 */
const base: StartAgent = { name: "c1", kind: "pi", pane: "w1:p1", timeoutMs: 60000, args: [] };

test("startAgent refuses timeouts herdr 0.9.3 refuses (<= 3000, > 300000, non-integer) without calling herdr", async () => {
  const h = useFakeHerdr(says({ id: "cli:x", result: {} }));
  for (const timeoutMs of [0, 1, 3000, 300001, 3000.5, Number.NaN]) {
    await assert.rejects(
      startAgent({ ...base, timeoutMs }),
      (e: unknown) => e instanceof CpdError && e.code === "invalid_timeout" && e.exitCode === 2,
      String(timeoutMs),
    );
  }
  assert.deepEqual(h.calls(), []);
});

test("startAgent passes the boundary timeouts 3001 and 300000 to herdr", async () => {
  const started = {
    id: "cli:x",
    result: { agent: { agent_session: { kind: "path", value: "/s.jsonl" } }, argv: ["pi"] },
  };
  const h = useFakeHerdr(says(started));
  for (const timeoutMs of [3001, 300_000]) await startAgent({ ...base, timeoutMs });
  assert.deepEqual(h.calls().map((c) => c[c.indexOf("--timeout") + 1]), ["3001", "300000"]);
});
