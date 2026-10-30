import assert from "node:assert/strict";
import { test } from "node:test";
import type { PromptSection } from "@earendil-works/pi-durable";
import { CpdError } from "../../lib/cpd.ts";
import { parseLine } from "../../lib/event.ts";
import type { FollowerHooks } from "../../lib/follower.ts";
import { POLICY_SECTION, policySection } from "../../lib/policy.ts";
import { TIME_ZONE_SECTION, zoneLine } from "../../lib/time.ts";
import type { Config } from "../../durable-worker/config.ts";
import type { Input } from "../../durable-worker/deliver.ts";
import { endReport, type EndReport, type Entry } from "../../durable-worker/report.ts";
import { runWorker, type Conversation, type Deps, type Submission } from "../../durable-worker/run.ts";
import { policyShow, shown } from "../answers.ts";
import { ev } from "../fake.ts";

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

const CONFIG: Config = { home: "/cpd-home", session: "s-worker", plan: "plan-1", attempt: "job-a1", cwd: "/work/tree", spec: "/cpd-home/run/plan-1/specs/job-a1.md", model: "anthropic/sonnet:high" };
const MODEL = { provider: "anthropic", modelId: "sonnet", thinkingLevel: "high" };
const NOW = new Date("2026-10-14T21:41:02.114Z");
const STORE = "/cpd-home/run/plan-1/durable/job-a1.sqlite";
const REPORT = "/cpd-home/run/plan-1/attempts/job-a1.report.json";
const FIRST = `Read ${CONFIG.spec} and follow it exactly. Your attempt id is ${CONFIG.attempt}.`;
const POLICY = policyShow("worker", { pr: "Open no pull request yourself." });
const FILES = [
  { path: "/work/tree/AGENTS.md", content: "agents rules" },
  { path: "/work/tree/CLAUDE.md", content: "claude rules" },
];

type Pending = { submission: Submission; settle(r: "done" | "unanswered"): void };

/** What pi-durable keeps in a store file: one Submission per requestId, whatever the process that asks. */
class Store {
  readonly submissions = new Map<string, Pending>();
  readonly submitted: Input[] = [];
  readonly entries: Entry[] = [];
  closeCalls = 0;
  private closeIt!: (v: unknown) => void;
  readonly closed = new Promise<unknown>((resolve) => {
    this.closeIt = resolve;
  });

  conversation(): Conversation & { close(): void } {
    return {
      submit: async (input) => {
        this.submitted.push(input);
        let pending = this.submissions.get(input.requestId);
        if (!pending) {
          let settle!: (r: "done" | "unanswered") => void;
          const result = new Promise<"done" | "unanswered">((resolve) => {
            settle = resolve;
          });
          pending = { submission: { wait: () => result }, settle };
          this.submissions.set(input.requestId, pending);
        }
        notify();
        return pending.submission;
      },
      entries: async () => [...this.entries],
      closed: this.closed,
      close: () => {
        this.closeCalls++;
      },
    };
  }

  /** Ends a turn of the request: the model answered, then the Submission settles. */
  settle(requestId: string, result: "done" | "unanswered" = "done"): void {
    const model = { role: "assistant", stopReason: "stop", provider: "anthropic", model: "sonnet", usage: { input: 10, output: 5, cacheRead: 1, cacheWrite: 2, cost: { total: 0.5 } } };
    this.entries.push({ kind: "pi.assistant", model: [model] });
    this.submissions.get(requestId)!.settle(result);
    notify();
  }

  /** pi-durable settles `closed` after a storage error. */
  close(error: unknown): void {
    this.closeIt(error);
    notify();
  }
}

type Rig = ReturnType<typeof rig>;

/** Contract §3.6: the moves `session state` accepts (the same state is a no-op, always legal). */
const MOVES: Record<string, readonly string[]> = {
  stopped: ["idle"],
  idle: ["working", "blocked", "waiting", "stopped"],
  working: ["idle", "blocked", "waiting", "stopped"],
  waiting: ["working", "idle", "blocked", "waiting", "stopped"],
  blocked: ["working", "idle", "waiting", "stopped"],
};
const legalMove = (from: string, to: string): boolean => from === to || (MOVES[from]?.includes(to) ?? false);

function rig(over: { store?: Store; sessionShowError?: CpdError; resolveModel?: Deps["resolveModel"]; sessionState?: string; entriesError?: Error; writeError?: Error; openGate?: Promise<void> } = {}) {
  const session = { state: over.sessionState ?? "stopped" };
  const store = over.store ?? new Store();
  const calls: string[][] = [];
  const opens: Parameters<Deps["open"]>[0][] = [];
  const reports: { file: string; report: EndReport }[] = [];
  const logs: string[] = [];
  const effects: string[] = [];
  const follower = { hooks: undefined as FollowerHooks | undefined, started: 0, stopped: 0 };
  const terminate = { handler: undefined as (() => void) | undefined };
  const ending = { code: undefined as number | undefined, error: undefined as unknown };

  const deps: Deps = {
    cpd: async (args) => {
      effects.push("cpd");
      calls.push([...args]);
      const line = args.join(" ");
      if (line === ["session", "show", CONFIG.session].join(" ")) {
        if (over.sessionShowError) throw over.sessionShowError;
        return shown({ registered_event: 41, state: session.state });
      }
      if (line === "policy show") return POLICY;
      const state = new RegExp(`^session state (working|stopped|idle( --reply)?|(waiting|blocked) --label .+) --session ${CONFIG.session}$`);
      if (state.test(line)) {
        const to = args[2]!;
        if (!legalMove(session.state, to)) throw new CpdError("illegal_transition", `Session ${CONFIG.session} cannot go from ${session.state} to ${to} (contract §3.6).`);
        session.state = to;
        notify();
        return { ok: true };
      }
      throw new Error(`unexpected cpd call: ${args.join(" ")}`);
    },
    resolveModel:
      over.resolveModel ??
      ((pattern) => {
        effects.push("resolveModel");
        assert.equal(pattern, CONFIG.model);
        return MODEL;
      }),
    contextFiles: (cwd) => {
      effects.push("contextFiles");
      assert.equal(cwd, CONFIG.cwd);
      return FILES;
    },
    open: async (o) => {
      effects.push("open");
      opens.push(o);
      notify();
      await over.openGate;
      const conversation = store.conversation();
      if (over.entriesError) conversation.entries = async () => Promise.reject(over.entriesError);
      return conversation;
    },
    follow: (hooks) => {
      effects.push("follow");
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
    writeReport: async (file, report) => {
      effects.push("writeReport");
      if (over.writeError) throw over.writeError;
      reports.push({ file, report });
      notify();
    },
    onTerminate: (handler) => {
      effects.push("onTerminate");
      terminate.handler = handler;
      notify();
    },
    homeDir: () => "/home/user",
    now: () => NOW,
    log: (line) => {
      logs.push(line);
    },
  };

  runWorker(CONFIG, deps).then(
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
  const states = () => calls.filter((a) => a[0] === "session" && a[1] === "state").map((a) => a.slice(2).join(" "));
  return { session, store, calls, opens, reports, logs, effects, follower, terminate, ending, until, states };
}

/** Waits until the runner is fully up: first message submitted, follower started, terminate handler set. */
async function up(r: Rig): Promise<FollowerHooks> {
  await r.until("the first message submitted", () => r.store.submitted.length >= 1);
  await r.until("the follower started", () => r.follower.started >= 1);
  await r.until("the terminate handler", () => r.terminate.handler);
  return r.follower.hooks!;
}

/** Ends the run the one way a driver does and waits for its exit code. */
async function stop(r: Rig): Promise<number> {
  r.terminate.handler!();
  return await r.until("the runner to end", () => r.ending.code);
}

const nudge = (id: number, prompt: string) => JSON.stringify(ev(id, "attempt-nudge", { session: CONFIG.session, data: { prompt } }));
const deliver = (hooks: FollowerHooks, line: string) => {
  assert.equal(parseLine(line).type, "event");
  hooks.onLine(line);
};
const stoppedArgs = ["session", "state", "stopped", "--session", CONFIG.session];

test("the first message names the Spec path and the Attempt id and is submitted with requestId = the Attempt id", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  assert.deepEqual(r.store.submitted, [{ content: FIRST, requestId: CONFIG.attempt, whenBusy: "followUp" }]);
  assert.equal(await stop(r), 143);
});

test("a runner started again for the same Attempt opens the same store file and submits the same requestId", { timeout: 20_000 }, async () => {
  const store = new Store();
  const first = rig({ store });
  await up(first);
  assert.equal(await stop(first), 143);

  const again = rig({ store });
  await again.until("the first message submitted again", () => store.submitted.length >= 2);
  await again.until("the follower started", () => again.follower.started >= 1);
  await again.until("the terminate handler", () => again.terminate.handler);
  assert.deepEqual(again.opens.map((o) => o.file), [STORE]);
  assert.deepEqual(first.opens.map((o) => o.file), [STORE]);
  assert.deepEqual(store.submitted.map((i) => i.requestId), [CONFIG.attempt, CONFIG.attempt]);
  assert.equal(store.submissions.size, 1, "one request, one Submission: nothing is sent twice");
  assert.equal(await stop(again), 143);
});

test("an unusable model is a usage error before anything is opened", { timeout: 20_000 }, async () => {
  const r = rig({
    resolveModel: () => {
      throw new CpdError("usage", "unusable model: nope");
    },
  });
  await whenTrue("the run to end", () => r.ending.error || r.ending.code);
  assert.equal(r.ending.code, undefined);
  const error = r.ending.error;
  if (!(error instanceof CpdError)) throw error;
  assert.equal(error.code, "usage");
  assert.deepEqual(r.effects, [], "no effect before the model is resolved");
});

test("the store is run/<plan>/durable/<attempt>.sqlite and the extension carries the policy, the context files and the time zone", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  assert.equal(r.opens.length, 1);
  const o = r.opens[0]!;
  assert.equal(o.file, STORE);
  assert.equal(o.cwd, CONFIG.cwd);
  assert.deepEqual(o.model, MODEL);
  const render = async (s: PromptSection) => s.render(undefined as never, undefined as never);
  const sections = o.extension.sections ?? [];
  assert.deepEqual(sections.map((s) => s.key), ["cpd-context-files", POLICY_SECTION, TIME_ZONE_SECTION]);
  assert.deepEqual(await Promise.all(sections.map(render)), [
    "## /work/tree/AGENTS.md\nagents rules\n\n## /work/tree/CLAUDE.md\nclaude rules",
    policySection(POLICY),
    zoneLine(NOW),
  ]);
  assert.deepEqual(r.calls.filter((a) => a[1] === "show").map((a) => a.slice(0, 2)), [["session", "show"], ["policy", "show"]]);
  assert.deepEqual(r.calls.find((a) => a[0] === "session" && a[1] === "show")!.slice(2), [CONFIG.session]);
  assert.equal(await stop(r), 143);
});

test("the follower starts at the registered_event of the Session", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await up(r);
  assert.equal(r.follower.started, 1);
  assert.equal(hooks.cursor(), 41);
  assert.equal(await stop(r), 143);
});

test("a delivered Event is submitted as follow-up input with requestId event:<id> and the cursor moves to it", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await up(r);
  deliver(hooks, nudge(42, "Carry on."));
  await r.until("the Event submitted", () => r.store.submitted.length >= 2);
  assert.deepEqual(r.store.submitted[1], { content: "Carry on.", requestId: "event:42", whenBusy: "followUp" });
  assert.equal(hooks.cursor(), 42);
  assert.equal(await stop(r), 143);
});

test("the Session is working while a submission is pending and idle when the last one settled", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await up(r);
  await r.until("working", () => r.states().includes(`working --session ${CONFIG.session}`));
  deliver(hooks, nudge(42, "More."));
  await r.until("the Event submitted", () => r.store.submitted.length >= 2);
  r.store.settle(CONFIG.attempt);
  await r.until("the first report", () => r.reports.length >= 1);
  r.store.settle("event:42");
  await r.until("idle", () => r.states().length >= 3);
  assert.deepEqual(r.states(), [`idle --session ${CONFIG.session}`, `working --session ${CONFIG.session}`, `idle --session ${CONFIG.session}`], "idle only once the last submission settled");
  assert.equal(await stop(r), 143);
});

test("the end report is written after every settle", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await up(r);
  r.store.settle(CONFIG.attempt);
  await r.until("a report", () => r.reports.length >= 1);
  assert.deepEqual(r.reports[0], { file: REPORT, report: endReport(r.store.entries) });
  assert.equal(r.reports[0]!.report.turns, 1);
  deliver(hooks, nudge(42, "Again."));
  await r.until("the Event submitted", () => r.store.submitted.length >= 2);
  r.store.settle("event:42");
  await r.until("a second report", () => r.reports.length >= 2);
  assert.deepEqual(r.reports[1], { file: REPORT, report: endReport(r.store.entries) });
  assert.equal(r.reports[1]!.report.turns, 2);
  assert.equal(await stop(r), 143);
});

test("a terminate signal reports stopped, stops the follower and returns 143 without closing the harness", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  assert.equal(await stop(r), 143);
  assert.equal(r.follower.stopped, 1);
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
  assert.equal(r.store.closeCalls, 0);
});

const assistant = (store: Store) => store.entries.push({ kind: "pi.assistant", model: [{ role: "assistant", stopReason: "toolUse", provider: "anthropic", model: "sonnet", usage: { input: 10, output: 5, cacheRead: 1, cacheWrite: 2, cost: { total: 0.5 } } }] });

test("terminate with a submission still pending writes the end report from the entries so far, before stopped", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  assistant(r.store);
  assert.equal(await stop(r), 143);
  assert.equal(r.reports.length, 1);
  assert.deepEqual(r.reports[0], { file: REPORT, report: endReport(r.store.entries) });
  assert.equal(r.reports[0]!.report.turns, 1);
  assert.equal(r.reports[0]!.report.stop_reason, "toolUse");
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
  assert.ok(r.effects.lastIndexOf("writeReport") < r.effects.lastIndexOf("cpd"), "the report is written before the final stopped call");
});

test("terminate before the conversation opened writes no report and still reports stopped", { timeout: 20_000 }, async () => {
  let openIt!: () => void;
  const r = rig({ openGate: new Promise<void>((resolve) => { openIt = resolve; }) });
  await r.until("the terminate handler", () => r.terminate.handler);
  await r.until("open called", () => r.opens.length >= 1);
  assert.equal(await stop(r), 143);
  openIt();
  assert.equal(r.reports.length, 0);
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
});

test("terminate with no assistant entry writes no report and does not throw", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  assert.equal(await stop(r), 143);
  assert.equal(r.reports.length, 0);
  assert.equal(r.ending.error, undefined);
  assert.ok(r.logs.length >= 1);
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
});

test("entries rejecting at terminate is logged; exit 143 and stopped sent", { timeout: 20_000 }, async () => {
  const r = rig({ entriesError: new Error("entries broke") });
  await up(r);
  assert.equal(await stop(r), 143);
  assert.equal(r.reports.length, 0);
  assert.ok(r.logs.some((l) => l.includes("entries broke")));
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
});

test("writeReport rejecting at terminate is logged; exit 143 and stopped sent", { timeout: 20_000 }, async () => {
  const r = rig({ writeError: new Error("disk full") });
  await up(r);
  assistant(r.store);
  assert.equal(await stop(r), 143);
  assert.ok(r.logs.some((l) => l.includes("disk full")));
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
});

test("terminate after a settled turn replaces the report with an equal one", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  r.store.settle(CONFIG.attempt);
  await r.until("a report", () => r.reports.length >= 1);
  assert.equal(await stop(r), 143);
  assert.equal(r.reports.length, 2);
  assert.deepEqual(r.reports[1], r.reports[0]);
});

test("a harness that closed after a storage error reports stopped and returns 1", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  r.store.close(new Error("storage: disk I/O error"));
  assert.equal(await r.until("the runner to end", () => r.ending.code), 1);
  assert.equal(r.follower.stopped, 1);
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
  assert.ok(r.logs.length >= 1, "the close is logged");
  assert.equal(r.store.closeCalls, 0);
});

test("a failure to read the Session reports stopped and returns 1", { timeout: 20_000 }, async () => {
  const r = rig({ sessionShowError: new CpdError("session_not_found", "no such session") });
  const code = await whenTrue("the runner to end", () => r.ending.code ?? (r.ending.error ? -1 : undefined));
  if (r.ending.error) throw r.ending.error;
  assert.equal(code, 1);
  assert.deepEqual(r.calls.at(-1), stoppedArgs);
  assert.equal(r.opens.length, 0, "nothing is opened");
  assert.equal(r.follower.started, 0);
});

test("the runner stays up after the first turn ends and resolves only on terminate, close or failure", { timeout: 20_000 }, async () => {
  const r = rig();
  const hooks = await up(r);
  r.store.settle(CONFIG.attempt);
  await r.until("idle", () => r.states().length >= 3);
  await r.until("a report", () => r.reports.length >= 1);
  assert.equal(r.ending.code, undefined, "still running after the turn ended");
  deliver(hooks, nudge(43, "The driver's nudge."));
  await r.until("the nudge submitted", () => r.store.submitted.length >= 2);
  assert.equal(r.ending.code, undefined, "still running and taking input");
  assert.equal(r.follower.stopped, 0);
  assert.equal(await stop(r), 143);
});

const idleArgs = `idle --session ${CONFIG.session}`;
const workingArgs = `working --session ${CONFIG.session}`;

test("a first start from stopped moves the Session idle, then working, and no move is refused", { timeout: 20_000 }, async () => {
  const r = rig();
  await up(r);
  await r.until("working", () => r.session.state === "working");
  assert.deepEqual(r.states(), [idleArgs, workingArgs]);
  assert.deepEqual(r.logs, []);
  assert.equal(await stop(r), 143);
});

for (const state of ["working", "idle", "waiting", "blocked"]) {
  test(`a restart whose Session is ${state} sends no idle and goes working`, { timeout: 20_000 }, async () => {
    const r = rig({ sessionState: state });
    await up(r);
    await r.until("working", () => r.session.state === "working");
    assert.deepEqual(r.states(), [workingArgs]);
    assert.deepEqual(r.logs, []);
    assert.equal(await stop(r), 143);
  });
}

test("the rig refuses stopped -> working with the real message", () => {
  assert.equal(legalMove("stopped", "working"), false);
  assert.equal(legalMove("stopped", "idle"), true);
  assert.equal(legalMove("working", "working"), true);
});

test("a show whose state is not a Session state is a bad_output and the runner ends 1", { timeout: 20_000 }, async () => {
  const r = rig({ sessionState: "asleep" });
  const code = await r.until("the runner to end", () => r.ending.code);
  assert.equal(code, 1);
  assert.equal(r.opens.length, 0);
});
