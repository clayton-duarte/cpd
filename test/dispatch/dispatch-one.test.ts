import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import {
  CPD_ENTRY,
  attemptRunArgs,
  automaticAttemptCount,
  dispatchOne,
  spawnDetached,
  tierForAttempt,
  type DispatchDeps,
  type Spawn,
} from "../../src/dispatch/dispatch-one.ts";
import type { Attempt } from "../../src/ledger/attempts.ts";
import type { Job } from "../../src/ledger/jobs.ts";
import { CpdError, refused } from "../../src/lib/errors.ts";
import { attemptLogPath, slotPath } from "../../src/lib/paths.ts";
import { acquireSlot } from "../../src/lib/slots.ts";
import { locatePi } from "../../src/lib/pi-install.ts";
import { formatTier } from "../../src/lib/tiers.ts";
import { sandboxEnv, tmpDir } from "../helpers/index.ts";
import { CATALOG } from "./fakes.ts";
import { writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const env = sandboxEnv();
process.env.CPD_HOME = env.CPD_HOME;
process.env.HOME = env.HOME;

const job = (over: Partial<Job> = {}): Job => ({
  id: "bd-1", key: "api", title: "Api", status: "queued", tier: "S3", reviewTier: "S4", goal: "g", context: "c",
  touches: ["a"], acceptance: ["a"], outOfScope: [], needs: [], closeReason: null, metadata: {}, ...over,
});

type Fake = { deps: DispatchDeps; calls: string[]; slotFree: boolean };

function fake(o: { job?: Job; attempts?: number; slotFree?: boolean; cas?: boolean; spawnThrows?: boolean; startThrows?: Error; notReady?: boolean } = {}): Fake {
  const calls: string[] = [];
  const f = { calls, slotFree: o.slotFree ?? true } as Fake;
  f.deps = {
    concurrency: 3,
    catalog: CATALOG,
    assertReady: () => calls.push("assertReady") && undefined,
    readJob: async () => o.job ?? job(),
    attemptCount: async () => o.attempts ?? 0,
    acquireSlot: async (who) => {
      calls.push(`acquireSlot ${who.attempt}`);
      return f.slotFree ? 1 : null;
    },
    releaseSlot: async (i, a) => {
      calls.push(`releaseSlot ${i} ${a}`);
      return true;
    },
    setSlotPid: async (i, a, pid) => void calls.push(`setSlotPid ${i} ${a} ${pid}`),
    startAttempt: async (key, tier) => {
      calls.push(`startAttempt ${key} ${tier}`);
      if (o.startThrows) throw o.startThrows;
      if (o.cas === false) return null;
      return { id: `${key}-a${(o.attempts ?? 0) + 1}` } as Attempt;
    },
    failAttempt: async (a, cause) => void calls.push(`failAttempt ${a} ${cause}`),
    spawnRun: (a, slot) => {
      calls.push(`spawnRun ${a} ${slot}`);
      if (o.spawnThrows) throw new CpdError("spawn_failed", "no", 1);
      return { pid: 4242 };
    },
  };
  return f;
}

// --- acceptance 2 ---

test("no free slot → no_slot (exit 3) and nothing is started", async () => {
  const f = fake({ slotFree: false });
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "no_slot" && e.exitCode === 3);
  assert.deepEqual(f.calls, ["assertReady", "acquireSlot api-a1"]);
});

test("CAS lost → cas_lost (exit 3) and the slot is released; nothing is spawned", async () => {
  const f = fake({ cas: false });
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "cas_lost" && e.exitCode === 3);
  assert.deepEqual(f.calls, ["assertReady", "acquireSlot api-a1", "startAttempt api S3", "releaseSlot 1 api-a1"]);
});

test("success: slot, then startAttempt at the Attempt Tier, then a detached spawn; the slot follows the spawned pid", async () => {
  const f = fake();
  const r = await dispatchOne("demo", "api", f.deps);
  assert.deepEqual(r, { attempt: "api-a1", slot: 1, pid: 4242, tier: "S3" });
  assert.deepEqual(f.calls, ["assertReady", "acquireSlot api-a1", "startAttempt api S3", "spawnRun api-a1 1", "setSlotPid 1 api-a1 4242"]);
});

test("the real spawn is exactly `cpd attempt run <id> --wf <wf> --slot <i>`, detached, output to the attempt log, unref'd", () => {
  const seen: { cmd: string; args: string[]; opts: any }[] = [];
  let unref = 0;
  const spawnFn: Spawn = (cmd, args, opts) => {
    seen.push({ cmd, args, opts });
    return { pid: 777, on: () => ({}) as never, unref: () => void unref++ };
  };
  assert.deepEqual(spawnDetached("demo", "api-a2", 2, spawnFn), { pid: 777 });
  assert.equal(seen.length, 1);
  assert.equal(seen[0]!.cmd, process.execPath);
  assert.deepEqual(seen[0]!.args, [CPD_ENTRY, "attempt", "run", "api-a2", "--wf", "demo", "--slot", "2"]);
  assert.deepEqual(seen[0]!.args, attemptRunArgs("demo", "api-a2", 2));
  assert.ok(CPD_ENTRY.endsWith("/bin/cpd.mjs"));
  assert.equal(seen[0]!.opts.detached, true);
  assert.equal(seen[0]!.opts.stdio[0], "ignore");
  assert.equal(typeof seen[0]!.opts.stdio[1], "number");
  assert.equal(seen[0]!.opts.stdio[1], seen[0]!.opts.stdio[2], "stdout and stderr share the log");
  assert.equal(unref, 1);
  assert.ok(existsSync(attemptLogPath("demo", "api-a2")), "the log file exists");
});

test("a spawn that yields no pid is spawn_failed", () => {
  const spawnFn: Spawn = () => ({ pid: undefined, on: () => ({}) as never, unref: () => {} });
  assert.throws(() => spawnDetached("demo", "api-a3", 0, spawnFn), (e: unknown) => e instanceof CpdError && e.code === "spawn_failed");
});

test("really detached: the child writes its result to logs/attempt-<id>.log and frees its slot when it cannot start", { timeout: 120_000 }, async (t) => {
  // No workflow exists, so `attempt run` fails at once; the slot must still be released and the JSON must land in the log.
  process.env.PI_CODING_AGENT_DIR = locatePi().agentDir;
  const home = process.env.CPD_HOME!;
  mkdirSync(home, { recursive: true });
  writeFileSync(
    join(home, "config.json"),
    JSON.stringify({
      version: 1, concurrency: 3, retryCap: 2, attemptTimeoutMinutes: 30, askTimeoutMinutes: 60, tickSeconds: 5, prPollSeconds: 120, herdrSession: null,
      tiers: { O: "github-copilot/claude-opus-5.5", S: "github-copilot/claude-sonnet-5.5", H: "github-copilot/claude-haiku-4.5" },
      leadTier: "O3", defaultReviewTier: "S4", reviewMode: "fork", leadPiPackages: [],
    }),
  );
  const slot = await acquireSlot(3, { wf: "ghost", attempt: "api-a1" });
  assert.notEqual(slot, null);
  const { pid } = spawnDetached("ghost", "api-a1", slot!);
  t.after(() => {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      // already gone
    }
  });
  const log = attemptLogPath("ghost", "api-a1");
  const deadline = Date.now() + 100_000;
  while (Date.now() < deadline && !(existsSync(log) && readFileSync(log, "utf8").includes("\n") && !existsSync(slotPath(slot!)))) await sleep(250);
  const out = JSON.parse(readFileSync(log, "utf8").trim().split("\n").at(-1)!);
  assert.equal(out.ok, false);
  assert.equal(out.error.code, "workflow_not_found");
  assert.equal(existsSync(slotPath(slot!)), false, "the slot was released by the detached process");
});

test("the Attempt id and Tier follow the ladder: Attempt n uses `next` applied n-1 times", async () => {
  const f = fake({ attempts: 2 });
  const r = await dispatchOne("demo", "api", f.deps);
  assert.equal(r.attempt, "api-a3");
  assert.equal(r.tier, "S5");
  assert.ok(f.calls.includes("startAttempt api S5"));
});

test("an explicit `job retry` Tier (retry_<n>_tier) is used for that Attempt", async () => {
  const f = fake({ attempts: 3, job: job({ metadata: { retry_4_tier: "H1" } }) });
  const r = await dispatchOne("demo", "api", f.deps);
  assert.deepEqual([r.attempt, r.tier], ["api-a4", "H1"]);
});

test("the ladder continues from the retry Tier, not from the Job Tier", () => {
  const j = job({ metadata: { retry_4_tier: "H1" } });
  assert.equal(formatTier(tierForAttempt(CATALOG, j, 5)!), "H2");
  assert.equal(formatTier(tierForAttempt(CATALOG, j, 3)!), "S5", "earlier Attempts are unchanged");
  assert.equal(automaticAttemptCount(j, 3), 3);
  assert.equal(automaticAttemptCount(j, 4), 1, "a retry starts a new automatic run");
  assert.equal(automaticAttemptCount(j, 6), 3);
  assert.equal(automaticAttemptCount(job(), 1), 1);
});

test("a later retry replaces an earlier one", () => {
  const j = job({ metadata: { retry_4_tier: "H1", retry_7_tier: "O2" } });
  assert.equal(formatTier(tierForAttempt(CATALOG, j, 7)!), "O2");
  assert.equal(formatTier(tierForAttempt(CATALOG, j, 8)!), "O3");
  assert.equal(formatTier(tierForAttempt(CATALOG, j, 6)!), "H3");
  assert.equal(automaticAttemptCount(j, 8), 2);
});

test("an exhausted ladder is refused before a slot is taken", async () => {
  const f = fake({ attempts: 1, job: job({ tier: "O5" }) });
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "ladder_exhausted" && e.exitCode === 3);
  assert.deepEqual(f.calls, ["assertReady"]);
});

test("a Workflow without a Lead is refused before a slot is taken", async () => {
  const f = fake();
  f.deps.assertReady = () => {
    throw refused("lead_not_attached", "no Lead");
  };
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "lead_not_attached");
  assert.deepEqual(f.calls, []);
});

test("startAttempt refusing (job_not_ready) releases the slot and propagates", async () => {
  const f = fake({ startThrows: refused("job_not_ready", "blocked") });
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "job_not_ready");
  assert.deepEqual(f.calls.at(-1), "releaseSlot 1 api-a1");
  assert.equal(f.calls.some((c) => c.startsWith("spawnRun")), false);
});

test("a spawn failure closes the started Attempt (Job failed), releases the slot and propagates", async () => {
  const f = fake({ spawnThrows: true });
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "spawn_failed");
  assert.deepEqual(f.calls.slice(-3), ["spawnRun api-a1 1", "failAttempt api-a1 dispatch_failed", "releaseSlot 1 api-a1"]);
});

test("a failure after the spawn (slot handoff) still releases the slot", async () => {
  const f = fake();
  f.deps.setSlotPid = async () => {
    throw refused("slot_not_held", "taken");
  };
  await assert.rejects(dispatchOne("demo", "api", f.deps), (e: unknown) => e instanceof CpdError && e.code === "slot_not_held");
  assert.equal(f.calls.at(-1), "releaseSlot 1 api-a1");
});

test("an invalid Job key is a usage error", async () => {
  await assert.rejects(dispatchOne("demo", "Bad Key", fake().deps), (e: unknown) => e instanceof CpdError && e.exitCode === 2);
});
