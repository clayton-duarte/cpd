import assert from "node:assert/strict";
import { test } from "node:test";
import type { Extension } from "@earendil-works/pi-durable";
import { CpdError } from "../../lib/cpd.ts";
import { POLICY_SECTION, policySection } from "../../lib/policy.ts";
import { TIME_ZONE_SECTION, zoneLine } from "../../lib/time.ts";
import type { Conversation } from "../../durable-host/run.ts";
import type { Config } from "../../durable-host/config.ts";
import { runHost, type Deps } from "../../durable-host/run.ts";
import { policyShow, shown } from "../answers.ts";

/** Every fake calls notify() when it sets a fact; a waiter re-checks its fact on each call. No timer, no polling. */
const waiters = new Set<() => void>();
function notify(): void {
  for (const wake of [...waiters]) wake();
}
/** Resolves when fn returns a truthy value; rejects when fn throws. A hang is caught by the test's timeout option. */
function whenTrue<T>(_what: string, fn: () => T | undefined | false): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const check = (): void => {
      try {
        const v = fn();
        if (!v) return;
        waiters.delete(check);
        resolve(v);
      } catch (e) {
        waiters.delete(check);
        reject(e);
      }
    };
    waiters.add(check);
    check();
  });
}

const CONFIG: Config = { home: "/cpd-home", session: "s-lead", cwd: "/work/tree", model: "anthropic/sonnet:high" };
const MODEL = { provider: "anthropic", modelId: "sonnet", thinkingLevel: "high" };
const NOW = new Date("2026-10-14T21:41:02.114Z");
const POLICY = policyShow("lead", { pr: "Open pull requests through cpd." });
const FILES = [
  { path: "/work/tree/AGENTS.md", content: "agents rules" },
  { path: "/work/tree/CLAUDE.md", content: "claude rules" },
];
const STOPPED = ["session", "state", "stopped", "--session", CONFIG.session];

const MOVES: Record<string, readonly string[]> = {
  stopped: ["idle"],
  idle: ["working", "blocked", "waiting", "stopped"],
  working: ["idle", "blocked", "waiting", "stopped"],
  waiting: ["working", "idle", "blocked", "waiting", "stopped"],
  blocked: ["working", "idle", "waiting", "stopped"],
};

function rig(over: { role?: string; state?: string } = {}) {
  const session = { state: over.state ?? "stopped" };
  const calls: string[][] = [];
  const opens: Parameters<Deps["open"]>[0][] = [];
  const logs: string[] = [];
  const submitted: unknown[] = [];
  const closeCalls = { n: 0 };
  const terminate = { handler: undefined as (() => void) | undefined };
  const ending = { code: undefined as number | undefined, error: undefined as unknown };
  let closeIt!: (v: unknown) => void;
  const closed = new Promise<unknown>((resolve) => {
    closeIt = resolve;
  });

  const deps: Deps = {
    guard: async () => {},
    cpd: async (args) => {
      calls.push([...args]);
      const line = args.join(" ");
      if (line === `session show ${CONFIG.session}`) return shown({ role: over.role ?? "lead", state: session.state });
      if (line === "policy show") return POLICY;
      const m = /^session state (working|stopped|idle) --session s-lead$/.exec(line);
      if (m) {
        const to = m[1]!;
        if (to !== session.state && !MOVES[session.state]?.includes(to)) throw new CpdError("illegal_transition", `cannot go from ${session.state} to ${to}`);
        session.state = to;
        notify();
        return { ok: true };
      }
      throw new Error(`unexpected cpd call: ${line}`);
    },
    resolveModel: () => MODEL,
    models: async () => [],
    contextFiles: () => FILES,
    open: async (o) => {
      opens.push(o);
      notify();
      const conversation: Conversation = {
        submit: async (input) => {
          submitted.push(input);
          throw new Error("the host submits nothing");
        },
        watch: async () => ({ snapshot: {}, start: () => {}, stop: async () => ({ reason: "stopped" }), closed: new Promise<unknown>(() => {}) }),
        abort: async () => {},
        abortSubmission: async () => "not_found",
        write: async () => ({}),
        install: () => {},
        entries: async () => [],
        compact: async () => 1,
        configure: async () => {},
        agent: async () => ({ thinkingLevel: "high" }),
        pageEntries: async () => ({ entries: [] }),
        closed,
      };
      return Object.assign(conversation, { close: () => closeCalls.n++ });
    },
    listen: async () => ({ close: async () => {} }),
    newEpoch: () => "epoch-1",
    onTerminate: (handler) => {
      terminate.handler = handler;
      notify();
    },
    homeDir: () => "/home/user",
    now: () => NOW,
    log: (line) => {
      logs.push(line);
    },
  };

  runHost(CONFIG, deps).then(
    (code) => {
      ending.code = code;
      notify();
    },
    (error) => {
      ending.error = error;
      notify();
    },
  );
  const until = <T>(what: string, fn: () => T | undefined | false): Promise<T> =>
    whenTrue(what, () => {
      if (ending.error) throw ending.error;
      return fn();
    });
  const states = () => calls.filter((a) => a[0] === "session" && a[1] === "state").map((a) => a[2]);
  const up = async () => {
    await until("the store opened", () => opens.length >= 1);
    await until("the terminate handler", () => terminate.handler);
  };
  const stop = async (): Promise<number> => {
    terminate.handler!();
    return await until("the host to end", () => ending.code);
  };
  return { calls, opens, logs, submitted, closeCalls, ending, closeIt: (v: unknown) => closeIt(v), until, states, up, stop };
}

test("runHost opens the store at run/host/<session>.sqlite", { timeout: 20_000 }, async () => {
  const r = rig();
  await r.up();
  const o = r.opens[0]!;
  assert.equal(o.file, "/cpd-home/run/host/s-lead.sqlite");
  assert.equal(o.cwd, CONFIG.cwd);
  assert.deepEqual(o.model, MODEL);
  const ext: Extension = o.extension;
  const keys = (ext.sections ?? []).map((s) => s.key);
  assert.deepEqual(keys, ["cpd-context-files", POLICY_SECTION, TIME_ZONE_SECTION]);
  const render = (i: number) => ext.sections![i]!.render(undefined as never, undefined as never);
  assert.equal(await render(0), "## /work/tree/AGENTS.md\nagents rules\n\n## /work/tree/CLAUDE.md\nclaude rules");
  assert.equal(await render(1), policySection(POLICY));
  assert.equal(await render(2), zoneLine(NOW));
  await r.stop();
});

test("runHost reports idle when the Session was stopped", { timeout: 20_000 }, async () => {
  const r = rig({ state: "stopped" });
  await r.up();
  await r.until("idle reported", () => r.states().includes("idle"));
  assert.equal(await r.stop(), 143);
  assert.deepEqual(r.states(), ["idle", "stopped"]);
});

test("runHost reports nothing at start when the Session was not stopped", { timeout: 20_000 }, async () => {
  for (const state of ["working", "idle", "waiting", "blocked"]) {
    const r = rig({ state });
    await r.up();
    assert.deepEqual(r.states(), [], state);
    await r.stop();
    assert.deepEqual(r.states(), ["stopped"], state);
  }
});

test("runHost submits nothing and writes no report at start", { timeout: 20_000 }, async () => {
  const r = rig();
  await r.up();
  await r.until("idle reported", () => r.states().includes("idle"));
  await r.stop();
  assert.deepEqual(r.submitted, []);
  assert.deepEqual(
    r.calls.filter((a) => a[0] !== "session" && a[0] !== "policy"),
    [],
    "no cpd call besides session show, policy show and the state reports",
  );
  assert.ok(r.opens.length === 1);
});

test("runHost refuses a Session whose role is worker with a usage error before opening the store", { timeout: 20_000 }, async () => {
  const r = rig({ role: "worker" });
  const error = await r.until("the host to refuse", () => r.ending.code ?? undefined).then(
    () => undefined,
    (e: unknown) => e,
  );
  assert.ok(error instanceof CpdError, String(error));
  assert.equal(error.code, "usage");
  assert.equal(r.opens.length, 0);
});

test("runHost reports stopped and returns 143 on terminate", { timeout: 20_000 }, async () => {
  const r = rig({ state: "working" });
  await r.up();
  assert.equal(await r.stop(), 143);
  assert.deepEqual(r.calls.at(-1), STOPPED);
  assert.equal(r.closeCalls.n, 0);
});

test("runHost reports stopped and returns 1 when the conversation closes", { timeout: 20_000 }, async () => {
  const r = rig({ state: "working" });
  await r.up();
  r.closeIt(new Error("storage: disk I/O error"));
  assert.equal(await r.until("the host to end", () => r.ending.code), 1);
  assert.deepEqual(r.calls.at(-1), STOPPED);
  assert.ok(r.logs.length >= 1, "the close is logged");
  assert.equal(r.closeCalls.n, 0);
});

test("runHost logs the reason and the error message of a closed conversation", { timeout: 20_000 }, async () => {
  const r = rig({ state: "working" });
  await r.up();
  r.closeIt({ reason: "failed", error: new Error("disk I/O error") });
  assert.equal(await r.until("the host to end", () => r.ending.code), 1);
  assert.ok(r.logs.some((l) => l.includes("failed") && l.includes("disk I/O error")), r.logs.join("\n"));
  assert.ok(!r.logs.some((l) => l.includes("[object Object]")), r.logs.join("\n"));
});
