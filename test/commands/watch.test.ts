import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readWatchFile } from "../../src/dispatch/watch.ts";
import { initWorkflow, setPhase } from "../../src/ledger/workflow.ts";
import { cpdLogPath, watchPath } from "../../src/lib/paths.ts";
import { cliSandbox } from "../ledger/helpers.ts";

// `cpd watch` at the CLI. The tick, the steps and the error policy are covered in test/dispatch/{watch,liveness,schedule}.test.ts.
const { repo, cli } = cliSandbox();

const iso = (ms: number) => new Date(ms).toISOString();
const holder = (pid: number, heartbeatAt = Date.now()) => ({ pid, startedAt: iso(heartbeatAt - 60_000), heartbeatAt: iso(heartbeatAt) });

/** A pid that is certainly not alive. */
function deadPid(): number {
  for (let pid = 4_000_000; ; pid++) {
    try {
      process.kill(pid, 0);
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === "ESRCH") return pid;
    }
  }
}

test("usage: --wf is required, unknown flags and arguments are refused (exit 2)", async () => {
  const none = await cli(["watch"]);
  assert.deepEqual([none.code, none.json.error.code], [2, "workflow_required"]);
  assert.equal((await cli(["watch", "--wf", "demo", "--tick", "1"])).code, 2);
  assert.equal((await cli(["watch", "--wf", "demo", "extra"])).code, 2);
  assert.equal((await cli(["watch", "--wf", "Bad_Id"])).code, 2);
});

test("an unknown Workflow is exit 4, and nothing is created for it", async () => {
  const r = await cli(["watch", "--wf", "nosuch"]);
  assert.deepEqual([r.code, r.json.error.code], [4, "workflow_not_found"]);
  assert.equal(existsSync(watchPath("nosuch")), false);
});

test("setup: a Workflow whose PR is merged (phase done)", async () => {
  await initWorkflow({ wf: "fin", repo, title: "Finished" });
  for (const phase of ["plan", "dispatch", "pr", "done"] as const) await setPhase("fin", phase);
});

test("a second cpd watch while one is alive is refused: watch_running (exit 3), and its watch.json is left as it was", async () => {
  mkdirSync(join(process.env.CPD_HOME!, "workflows", "fin"), { recursive: true });
  const first = holder(process.pid); // this test process is alive
  writeFileSync(watchPath("fin"), JSON.stringify(first));
  const r = await cli(["watch", "--wf", "fin"]);
  assert.equal(r.code, 3, r.stdout + r.stderr);
  assert.equal(r.json.error.code, "watch_running");
  assert.equal(r.json.error.details.pid, process.pid);
  assert.deepEqual(JSON.parse(readFileSync(watchPath("fin"), "utf8")), first);
});

test("a stale watch.json (dead pid) is taken over; at phase done the watch exits 0 and removes it", { timeout: 300_000 }, async () => {
  writeFileSync(watchPath("fin"), JSON.stringify(holder(deadPid())));
  const r = await cli(["watch", "--wf", "fin"]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.deepEqual({ ok: r.json.ok, wf: r.json.wf, phase: r.json.phase }, { ok: true, wf: "fin", phase: "done" });
  assert.equal(readWatchFile("fin"), null, "released on exit");
  const events = readFileSync(cpdLogPath("fin"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  assert.ok(events.some((e) => e.event === "watch_takeover"), "the takeover is logged");
  assert.ok(events.some((e) => e.event === "watch_done"));
});

test("a live pid with a stale heartbeat (a reused pid) is taken over as well", { timeout: 300_000 }, async () => {
  writeFileSync(watchPath("fin"), JSON.stringify(holder(process.pid, Date.now() - 60 * 60_000)));
  const r = await cli(["watch", "--wf", "fin"]);
  assert.equal(r.code, 0, r.stdout + r.stderr);
  assert.equal(r.json.phase, "done");
});
