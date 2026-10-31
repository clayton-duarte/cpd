import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import type { FollowerHooks } from "../../lib/follower.ts";
import type { Config } from "../../durable-host/config.ts";
import type { Input } from "../../durable-host/deliver.ts";
import { runHost, type Conversation, type Deps } from "../../durable-host/run.ts";
import { ev } from "../fake.ts";
import { policyShow, shown } from "../answers.ts";

/** Every fake calls notify() when it sets a fact; a waiter re-checks its fact on each call. No timer, no polling. */
const waiters = new Set<() => void>();
function notify(): void {
  for (const wake of [...waiters]) wake();
}
function whenTrue<T>(fn: () => T | undefined | false): Promise<T> {
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
const POLICY = policyShow("lead", {});

const line = (id: number, kind: string, over: Record<string, unknown> = {}) => JSON.stringify(ev(id, kind, { session: CONFIG.session, ...over }));
const entry = (id: number, kind: string) => ({ kind: "cpd-event", data: { id, kind, delivered_to: CONFIG.session } });

function rig(over: { stored?: readonly unknown[] } = {}) {
  const session = { state: "idle" };
  const calls: string[][] = [];
  const logs: string[] = [];
  const submitted: Input[] = [];
  const written: unknown[] = [];
  const finish: ((v: "done" | "unanswered") => void)[] = [];
  const follower = { hooks: undefined as FollowerHooks | undefined, started: 0, stopped: 0 };
  const terminate = { handler: undefined as (() => void) | undefined };
  const ending = { code: undefined as number | undefined, error: undefined as unknown };

  const deps: Deps = {
    cpd: async (args) => {
      calls.push([...args]);
      const text = args.join(" ");
      if (text === `session show ${CONFIG.session}`) return shown({ role: "lead", state: session.state, registered_event: 100 });
      if (text === "policy show") return POLICY;
      const m = /^session state (working|idle|stopped)(?: --reply)? --session s-lead$/.exec(text);
      if (m) {
        session.state = m[1]!;
        notify();
        return { ok: true };
      }
      throw new Error(`unexpected cpd call: ${text}`);
    },
    resolveModel: () => MODEL,
    contextFiles: () => [],
    open: async () => {
      const conversation: Conversation = {
        submit: async (input) => {
          submitted.push(input);
          notify();
          return {
            wait: () =>
              new Promise<"done" | "unanswered">((resolve) => {
                finish.push(resolve);
                notify();
              }),
          };
        },
        write: async (w) => {
          written.push(w);
          return {};
        },
        install: () => {},
        entries: async () => over.stored ?? [],
        closed: new Promise<unknown>(() => {}),
      };
      return conversation;
    },
    follow: (hooks) => {
      follower.hooks = hooks;
      return {
        start: () => {
          follower.started++;
          notify();
        },
        stop: () => {
          follower.stopped++;
        },
      };
    },
    onTerminate: (handler) => {
      terminate.handler = handler;
      notify();
    },
    homeDir: () => "/home/user",
    now: () => NOW,
    log: (text) => {
      logs.push(text);
      notify();
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
  const until = <T>(fn: () => T | undefined | false): Promise<T> =>
    whenTrue(() => {
      if (ending.error) throw ending.error;
      return fn();
    });
  const states = () => calls.filter((a) => a[0] === "session" && a[1] === "state").map((a) => a[2]);
  const stateCalls = () => calls.filter((a) => a[0] === "session" && a[1] === "state");
  const up = async (): Promise<FollowerHooks> => {
    await until(() => follower.started >= 1 && terminate.handler);
    return follower.hooks!;
  };
  const stop = async (): Promise<number> => {
    terminate.handler!();
    return await until(() => ending.code);
  };
  return { logs, submitted, written, stateCalls, finish, follower, ending, until, states, up, stop };
}

test("runHost starts the follower at the cursor read from the stored entries", { timeout: 20_000 }, async () => {
  const r = rig({ stored: [entry(130, "job-failed"), entry(120, "pr-feedback"), { kind: "cpd-event", data: { id: 400, kind: "job-failed", delivered_to: "the-source" } }] });
  const hooks = await r.up();
  assert.equal(hooks.cursor(), 130);
  await r.stop();
  assert.equal(r.follower.stopped, 1);

  const none = rig({ stored: [entry(30, "job-failed")] });
  assert.equal((await none.up()).cursor(), 100, "registered_event is greater");
  await none.stop();
});

test("runHost submits the input of a followed Event and reports working then idle", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  assert.equal(hooks.onLine(line(150, "pr-feedback")), true);
  await r.until(() => r.submitted.length >= 1);
  assert.deepEqual(r.submitted, [{ content: "text of pr-feedback 150", requestId: "event:150", whenBusy: "steer", entry: entry(150, "pr-feedback") }]);
  assert.equal(hooks.cursor(), 150);
  await r.until(() => r.states().includes("working"));
  r.finish[0]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.states(), ["working", "idle"]);
  await r.stop();
});

test("runHost submits an Event once when the follower delivers it twice", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(151, "job-failed"));
  hooks.onLine(line(151, "job-failed"));
  await r.until(() => r.submitted.length >= 1);
  hooks.onLine(line(152, "job-failed"));
  await r.until(() => r.submitted.length >= 2);
  assert.deepEqual(
    r.submitted.map((i) => i.requestId),
    ["event:151", "event:152"],
  );
  await r.stop();
});

test("runHost reports idle only when no submission is pending", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(160, "job-failed"));
  hooks.onLine(line(161, "job-failed"));
  await r.until(() => r.finish.length >= 2);
  r.finish[0]!("done");
  r.finish[1]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.states(), ["working", "idle"]);
  await r.stop();
});

test("runHost logs a follower failure and keeps running", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onFailure(new CpdError("tool_failed", "events follow exited 1"));
  await r.until(() => r.logs.some((l) => l.includes("events follow exited 1")));
  assert.equal(r.ending.code, undefined);
  hooks.onLine(line(170, "job-failed"));
  await r.until(() => r.submitted.length >= 1);
  assert.equal(await r.stop(), 143);
});

const IDLE = ["session", "state", "idle", "--session", CONFIG.session];
const IDLE_REPLY = ["session", "state", "idle", "--reply", "--session", CONFIG.session];

test("runHost reports idle with reply after a run started by a message from the User", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(200, "session-message", { data: { text: "hello", from: null } }));
  await r.until(() => r.finish.length >= 1);
  r.finish[0]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.stateCalls().at(-1), IDLE_REPLY);
  await r.stop();
});

test("runHost reports idle without reply after a run of Events and messages from Sessions", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(210, "pr-feedback"));
  hooks.onLine(line(211, "job-failed"));
  await r.until(() => r.finish.length >= 2);
  r.finish[0]!("done");
  r.finish[1]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.stateCalls().at(-1), IDLE);
  await r.stop();

  const peer = rig();
  const peerHooks = await peer.up();
  peerHooks.onLine(line(212, "session-message", { data: { text: "hi", from: "s9", wake: false } }));
  await peer.until(() => peer.finish.length >= 1);
  peer.finish[0]!("done");
  await peer.until(() => peer.states().includes("idle"));
  assert.deepEqual(peer.stateCalls().at(-1), IDLE);
  await peer.stop();

  const user = rig();
  const userHooks = await user.up();
  userHooks.onLine(line(213, "session-message", { data: { text: "hello", from: null } }));
  await user.until(() => user.finish.length >= 1);
  user.finish[0]!("done");
  await user.until(() => user.states().includes("idle"));
  assert.deepEqual(user.stateCalls().at(-1), IDLE_REPLY, "contrast: the User's message makes a reply run");
  await user.stop();
});

test("runHost makes a run a reply run when a message from the User joins it", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(220, "pr-feedback"));
  await r.until(() => r.finish.length >= 1);
  hooks.onLine(line(221, "session-message", { data: { text: "typed during the run", from: null } }));
  await r.until(() => r.finish.length >= 2);
  r.finish[0]!("done");
  r.finish[1]!("done");
  await r.until(() => r.states().includes("idle"));
  assert.deepEqual(r.stateCalls().at(-1), IDLE_REPLY);
  await r.stop();
});

test("runHost reports reply once and the next run without it", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await r.up();
  hooks.onLine(line(230, "session-message", { data: { text: "hello", from: null } }));
  await r.until(() => r.finish.length >= 1);
  r.finish[0]!("done");
  await r.until(() => r.states().filter((s) => s === "idle").length >= 1);
  hooks.onLine(line(231, "pr-feedback"));
  await r.until(() => r.finish.length >= 2);
  r.finish[1]!("done");
  await r.until(() => r.states().filter((s) => s === "idle").length >= 2);
  assert.deepEqual(
    r.stateCalls().filter((a) => a[2] === "idle"),
    [IDLE_REPLY, IDLE],
  );
  await r.stop();
});
