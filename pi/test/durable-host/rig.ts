import assert from "node:assert/strict";
import type { FollowerHooks } from "../../lib/follower.ts";
import type { Config } from "../../durable-host/config.ts";
import type { Input } from "../../durable-host/deliver.ts";
import type { Connection } from "../../durable-host/server.ts";
import type { Extension } from "@earendil-works/pi-durable";
import { runHost, type Conversation, type Deps, type EventStream } from "../../durable-host/run.ts";
import { ev } from "../fake.ts";
import { policyShow, shown } from "../answers.ts";

/** The rig of the host tests (follow.test.ts, socket.test.ts): fakes for every `Deps` member. Every fake calls notify() when it sets a fact; no timer. */
const waiters = new Set<() => void>();
export function notify(): void {
  for (const wake of [...waiters]) wake();
}
export function whenTrue<T>(fn: () => T | undefined | false): Promise<T> {
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

export const CONFIG: Config = { home: "/cpd-home", session: "s-lead", cwd: "/work/tree", model: "anthropic/sonnet:high" };
export const MODEL = { provider: "anthropic", modelId: "sonnet", thinkingLevel: "high" };
export const NOW = new Date("2026-10-14T21:41:02.114Z");
export const POLICY = policyShow("lead", {});
export const EPOCH = "epoch-1";

export const line = (id: number, kind: string, over: Record<string, unknown> = {}) => JSON.stringify(ev(id, kind, { session: CONFIG.session, ...over }));
export const entry = (id: number, kind: string) => ({ kind: "cpd-event", data: { id, kind, delivered_to: CONFIG.session } });

/** A stream the test drives: `push` feeds a batch to the started listener, `settle` ends `closed`. `stop` settles `closed` with `{ reason: "stopped" }` as pi-durable does. */
export type FakeStream = EventStream & {
  started: number;
  stopped: number;
  listener: ((events: readonly unknown[]) => Promise<void>) | undefined;
  push(events: readonly unknown[]): Promise<void>;
  settle(end: unknown): void;
};

/** A client connection: `receive` feeds a line to the server, `received` is every line the host wrote, parsed. */
export type FakeClient = {
  conn: Connection;
  closed: number;
  lines: string[];
  received(): Record<string, unknown>[];
  send(message: Record<string, unknown>): void;
  hangup(): void;
};

export type RigOptions = {
  stored?: readonly unknown[];
  config?: Config;
  /** Answers by `<a>-<b>` of `cpd <a> <b>`; when given, an unlisted call answers `{ ok: true }`. */
  answers?: Record<string, Record<string, unknown>>;
  sessionOver?: Record<string, unknown>;
  /** A `submit` with `whenBusy: "reject"` fails with pi-durable's ConversationBusy while true. */
  busy?: { value: boolean };
  /** What `Deps.guard` does: resolves by default; rejects with the given error. */
  guard?: Error;
  abortSubmission?: "aborted" | "already_placed" | "settled" | "not_found";
};

export function rig(over: RigOptions = {}) {
  const config = over.config ?? CONFIG;
  const session = { state: "idle" };
  const calls: string[][] = [];
  const logs: string[] = [];
  const submitted: Input[] = [];
  const written: unknown[] = [];
  /** Facts in the order they happened: `open`, `watch`, `listen`, `close`, `bye`, `write:<kind>`, `submit:<requestId>`, `state:<state>`. */
  const order: string[] = [];
  const finish: ((v: "done" | "unanswered") => void)[] = [];
  const follower = { hooks: undefined as FollowerHooks | undefined, started: 0, stopped: 0 };
  const terminate = { handler: undefined as (() => void) | undefined };
  const ending = { code: undefined as number | undefined, error: undefined as unknown };
  const opened = { n: 0 };
  /** The extension each `Deps.open` call received, in order (the code mode wiring test reads it). */
  const extensions: Extension[] = [];
  const streams: FakeStream[] = [];
  const listens: { path: string; accept: Parameters<Deps["listen"]>[1] }[] = [];
  const listener = { closes: 0 };
  const aborts: { background: boolean }[] = [];
  const abortedSubmissions: number[] = [];
  const clients: FakeClient[] = [];
  let closeConversation!: (v: unknown) => void;
  const conversationClosed = new Promise<unknown>((resolve) => {
    closeConversation = resolve;
  });
  let nextSubmission = 41;

  const watch = async (): Promise<FakeStream> => {
    const n = streams.length + 1;
    let settle!: (v: unknown) => void;
    const closed = new Promise<unknown>((resolve) => {
      settle = resolve;
    });
    const stream: FakeStream = {
      snapshot: { type: "snapshot", n },
      started: 0,
      stopped: 0,
      listener: undefined,
      closed,
      start: (l) => {
        stream.started++;
        stream.listener = l;
        notify();
      },
      stop: async () => {
        stream.stopped++;
        settle({ reason: "stopped" });
        notify();
        return { reason: "stopped" };
      },
      push: async (events) => {
        assert.ok(stream.listener !== undefined, `stream ${n} was not started: the host never registered its listener`);
        await stream.listener!(events);
      },
      settle: (end) => {
        settle(end);
        notify();
      },
    };
    streams.push(stream);
    order.push("watch");
    notify();
    return stream;
  };

  /** Each `Deps.guard` call: its path and how many cpd calls and ordered facts came before it. */
  const guards: { path: string; calls: number; facts: number }[] = [];
  const deps: Deps = {
    guard: async (path) => {
      guards.push({ path, calls: calls.length, facts: order.length });
      order.push("guard");
      notify();
      if (over.guard !== undefined) throw over.guard;
    },
    cpd: async (args) => {
      calls.push([...args]);
      const text = args.join(" ");
      if (text === `session show ${config.session}`) return shown({ role: "lead", state: session.state, registered_event: 100, ...over.sessionOver });
      if (text === "policy show") return POLICY;
      const m = /^session state (working|idle|stopped)(?: --reply)? --session s-lead$/.exec(text);
      if (m) {
        session.state = m[1]!;
        order.push(`state:${m[1]}`);
        notify();
        return { ok: true };
      }
      const given = over.answers?.[args.slice(0, 2).join("-")];
      if (given !== undefined) return given;
      if (over.answers !== undefined) return { ok: true };
      throw new Error(`unexpected cpd call: ${text}`);
    },
    resolveModel: () => MODEL,
    contextFiles: () => [],
    open: async (o) => {
      opened.n++;
      extensions.push(o.extension);
      order.push("open");
      notify();
      const conversation: Conversation = {
        submit: async (input) => {
          if (over.busy?.value && input.whenBusy === "reject") {
            const { ConversationBusy } = await import("@earendil-works/pi-durable");
            throw new ConversationBusy("root" as never);
          }
          submitted.push(input);
          order.push(`submit:${input.requestId}`);
          notify();
          const id = nextSubmission++;
          return {
            id,
            status: async () => "queued",
            wait: () =>
              new Promise<"done" | "unanswered">((resolve) => {
                finish.push(resolve);
                notify();
              }),
          };
        },
        write: async (w) => {
          written.push(w);
          order.push(`write:${w.entry.kind}`);
          return {};
        },
        install: () => {},
        entries: async () => over.stored ?? [],
        closed: conversationClosed,
        watch,
        abort: async (opts) => {
          aborts.push(opts);
          notify();
        },
        abortSubmission: async (id) => {
          abortedSubmissions.push(id);
          notify();
          return over.abortSubmission ?? "aborted";
        },
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
    listen: async (path, accept) => {
      listens.push({ path, accept });
      order.push("listen");
      notify();
      return {
        close: async () => {
          listener.closes++;
          order.push("close");
          notify();
        },
      };
    },
    newEpoch: () => EPOCH,
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

  runHost(config, deps).then(
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

  /** Lets every pending promise chain of the fakes run (no fake uses a timer or I/O, so one turn of the event loop settles them all): not a wait for time. */
  const drain = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));
  /** The store was opened (or the host failed first) and everything that follows has settled. */
  const ready = async (): Promise<void> => {
    await whenTrue(() => opened.n >= 1 || ending.error !== undefined || ending.code !== undefined);
    await drain();
  };
  /** Ends the host when it still runs (a test's cleanup). */
  const shutdown = async (): Promise<void> => {
    if (ending.code !== undefined || ending.error !== undefined) return;
    terminate.handler?.();
    await drain();
  };
  /** A fake client: the test calls the `accept` function `Deps.listen` was given. */
  const connect = (): FakeClient => {
    const accept = listens[0]?.accept;
    assert.ok(accept !== undefined, "the host did not listen");
    const lines: string[] = [];
    const client: FakeClient = {
      closed: 0,
      lines,
      conn: {
        write: (l) => {
          lines.push(l);
          if (JSON.parse(l).type === "bye") order.push("bye");
          notify();
        },
        buffered: () => 0,
        close: () => {
          client.closed++;
        },
      },
      received: () => lines.map((l) => JSON.parse(l) as Record<string, unknown>),
      send: (m) => handler.receive(JSON.stringify(m)),
      hangup: () => handler.closed(),
    };
    const handler = accept(client.conn);
    clients.push(client);
    return client;
  };
  /** A connected client that said hello and attached (without a cursor, unless given). */
  const attach = async (cursor?: { epoch: string; seq: number }): Promise<FakeClient> => {
    const client = connect();
    client.send({ type: "hello", protocol: 1, min_protocol: 1, client: "test" });
    client.send({ type: "attach", req: "attach-1", ...(cursor === undefined ? {} : { cursor }) });
    await drain();
    return client;
  };
  return {
    guards, logs, submitted, written, stateCalls, finish, follower, ending, until, states, up, stop,
    calls, order, streams, listens, listener, aborts, abortedSubmissions, clients, opened, extensions,
    closeConversation: (v: unknown) => closeConversation(v),
    drain, ready, shutdown, connect, attach,
  };
}

/** Runs `fn` over a fresh rig and always ends the host after it. */
export async function withHost(over: RigOptions, fn: (r: ReturnType<typeof rig>) => Promise<void>): Promise<void> {
  const r = rig(over);
  try {
    await fn(r);
  } finally {
    await r.shutdown();
  }
}
