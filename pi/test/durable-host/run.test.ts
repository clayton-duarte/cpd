import assert from "node:assert/strict";
import { test } from "node:test";
import type { Extension, ToolHooks } from "@earendil-works/pi-durable";
import { CpdError } from "../../lib/cpd.ts";
import { ENTRY_TYPE } from "../../lib/loop-guard.ts";
import { POLICY_SECTION, policySection } from "../../lib/policy.ts";
import { TIME_ZONE_SECTION, zoneLine } from "../../lib/time.ts";
import type { Conversation } from "../../durable-host/run.ts";
import type { Config } from "../../durable-host/config.ts";
import { runHost, type Deps } from "../../durable-host/run.ts";
import { policyShow, shown } from "../answers.ts";
import { line, withHost } from "./rig.ts";

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

function rig(over: { role?: string; state?: string; firstMessage?: boolean } = {}) {
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
      // Answers the startup first-message read of a fresh durable Plan Lead (contract §6.13 step 7): the text and its Event id.
      if (over.firstMessage === true && line === `session first-message --session ${CONFIG.session}`) return { ok: true, text: "Build the thing.", event: 120 };
      throw new Error(`unexpected cpd call: ${line}`);
    },
    resolveModel: () => MODEL,
    models: async () => [],
    contextFiles: () => FILES,
    validatePackage: async () => undefined,
    open: async (o) => {
      opens.push(o);
      notify();
      const conversation: Conversation = {
        submit: async (input) => {
          submitted.push(input);
          // The first-turn test needs the submission to return so the host can finish starting; the other tests keep the refusal of an unexpected submit.
          if (over.firstMessage === true) return { id: 1, status: async () => "queued", wait: async () => "done" };
          throw new Error("the host submits nothing");
        },
        watch: async () => ({ value: {}, start: () => {}, stop: async () => ({ kind: "stopped" }), closed: new Promise<unknown>(() => {}) }),
        abort: async () => {},
        abortSubmission: async () => "not_found",
        write: async () => ({}),
        install: () => {},
        configure: async () => {},
        entries: async () => [],
        compact: async () => 1,
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
  // Lets every pending promise chain of the fakes run: no fake uses a timer or I/O, so one turn of the event loop settles them all.
  const drain = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));
  const states = () => calls.filter((a) => a[0] === "session" && a[1] === "state").map((a) => a[2]);
  const up = async () => {
    await until("the store opened", () => opens.length >= 1);
    await until("the terminate handler", () => terminate.handler);
  };
  const stop = async (): Promise<number> => {
    terminate.handler!();
    return await until("the host to end", () => ending.code);
  };
  return { calls, opens, logs, submitted, closeCalls, ending, closeIt: (v: unknown) => closeIt(v), until, states, up, stop, drain };
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
  // P1.9 adds the role's skills section and the code mode section beside the base three.
  assert.deepEqual(keys, ["cpd-context-files", POLICY_SECTION, TIME_ZONE_SECTION, "cpd-skills", "codemode"]);
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

// The startup first turn (contract §6.13 step 7, hs-n658/hs-d812): a fresh non-by-user Plan Lead has no input of its
// own and no pi argv first message, so the host reads `cpd session first-message` and submits its text as the first turn.
test("a fresh durable Plan Lead runs its first turn at birth", { timeout: 20_000 }, async () => {
  const r = rig({ firstMessage: true });
  await r.up();
  await r.until("idle reported", () => r.states().includes("idle"));
  await r.drain();
  assert.ok(
    r.calls.some((a) => a.join(" ") === `session first-message --session ${CONFIG.session}`),
    `the host reads the first message at startup: ${JSON.stringify(r.calls)}`,
  );
  assert.equal(r.submitted.length, 1, `one turn-starting input: ${JSON.stringify(r.submitted)}`);
  const input = r.submitted[0] as { content: unknown; entry: { kind: string; data: Record<string, unknown> } };
  assert.equal(input.content, "Build the thing.", "the input's content is the first message");
  assert.equal(input.entry.kind, "cpd-first-message", "the input is tagged as the first message");
  assert.equal(input.entry.data.event, 120, "the first message's Event id counts it");
  assert.equal(input.entry.data.delivered_to, CONFIG.session, "the first message is delivered to this Session");
  await r.stop();
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

// The loop gate at contract §6.16's single submit: a client `submit` and a follower Event alike are submissions the host makes.
test("the host registers every client submit and follower submission at the submit gate", { timeout: 20_000 }, async () => {
  // The bound is 1, so the second registered submission trips the loop guard and the host appends its entry.
  await withHost({ sessionOver: { loop_guard_turns: 1 } }, async (r) => {
    const hooks = await r.up();
    const client = await r.attach();
    client.send({ type: "submit", req: "r1", text: "hello", when_busy: "followUp" });
    await r.until(() => r.submitted.length >= 1);
    hooks.onLine(line(300, "job-failed"));
    await r.until(() => r.submitted.length >= 2);
    await r.drain();
    assert.ok(
      r.written.some((w) => (w as { entry?: { kind?: unknown } }).entry?.kind === ENTRY_TYPE),
      `the gate must count both submissions and trip; writes were ${JSON.stringify(r.written)}`,
    );
  });
});

/** The host's `beforeTool` hook of the extension the host opened, called as pi-durable calls it. */
const toolHook = (e: Extension) => {
  const reg = e.hooks?.[0];
  assert.ok(reg, "the host extension has its tool hook first");
  return (name: string, args: Record<string, string>) => (reg.handlers as ToolHooks).beforeTool({ type: "toolCall", id: "c", name, arguments: args }, undefined as never, undefined as never);
};

test("a submit resets the per-turn loop counters", { timeout: 20_000 }, async () => {
  await withHost({}, async (r) => {
    await r.up();
    const client = await r.attach();
    const hook = toolHook(r.extensions[0]!);
    const read = () => hook("read", { path: "/x" });
    for (let i = 0; i < 6; i++) assert.equal(await read(), undefined, `read ${i + 1} runs`);
    client.send({ type: "submit", req: "r1", text: "hello", when_busy: "followUp" });
    await r.until(() => r.submitted.length >= 1);
    // the submission starts a turn: the six reads before it no longer count, so 12 more run and the 13th is refused
    for (let i = 0; i < 12; i++) assert.equal(await read(), undefined, `read ${i + 1} of the new turn runs`);
    const refused = await read();
    assert.match(String((refused as { block?: string } | undefined)?.block), /more than 12 consecutive read-only tool calls/);
  });
});

/** A host with bound 1 whose breaker is tripped and whose run has settled: nothing pending, the next turn start is held. */
async function tripped(r: Parameters<Parameters<typeof withHost>[1]>[0]) {
  const hooks = await r.up();
  const client = await r.attach();
  client.send({ type: "submit", req: "r1", text: "hello", when_busy: "followUp" });
  await r.until(() => r.submitted.length >= 1);
  hooks.onLine(line(300, "job-failed"));
  await r.until(() => r.submitted.length >= 2);
  await r.drain();
  assert.ok(r.written.some((w) => (w as { entry?: { kind?: unknown } }).entry?.kind === ENTRY_TYPE), "the second start tripped the breaker");
  for (const done of r.finish.splice(0)) done("done");
  await r.drain();
  return { hooks, client };
}
const clearEntries = (r: { written: unknown[] }) =>
  r.written.filter((w) => {
    const e = (w as { entry?: { kind?: unknown; data?: { tripped?: unknown } } }).entry;
    return e?.kind === ENTRY_TYPE && e.data?.tripped === false;
  });

test("a cpd-chat User prompt clears a tripped host", { timeout: 20_000 }, async () => {
  await withHost({ sessionOver: { loop_guard_turns: 1 } }, async (r) => {
    const { client } = await tripped(r);
    client.send({ type: "submit", req: "r2", text: "carry on", when_busy: "followUp" });
    await r.drain();
    assert.equal(clearEntries(r).length, 1, "the User's prompt appends the cleared entry");
    assert.equal(r.submitted.length, 3, "the prompt is submitted, not held");
  });
});

test("a session-message without a sender does not clear a tripped host", { timeout: 20_000 }, async () => {
  await withHost({ sessionOver: { loop_guard_turns: 1 } }, async (r) => {
    const { hooks } = await tripped(r);
    hooks.onLine(line(301, "session-message", { data: { text: "hi", from: null } }));
    await r.drain();
    assert.equal(clearEntries(r).length, 0, "no cleared entry");
    assert.equal(r.submitted.length, 2, "the sender-less message is held, not submitted");
  });
});
