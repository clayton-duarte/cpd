import { randomUUID } from "node:crypto";
import { ConversationBusy, GenerationTask, hook, type Extension } from "@earendil-works/pi-durable";
import type { FollowerHooks } from "../lib/follower.ts";
import { parseLine } from "../lib/event.ts";
import { cursorFrom } from "./cursor.ts";
import { inputOf, type Input } from "./deliver.ts";
import { CpdError } from "../lib/cpd.ts";
import { policySection } from "../lib/policy.ts";
import { parseSession } from "../lib/run.ts";
import { Reporter, stateArgs, type Wanted } from "../lib/state.ts";
import { zoneLine } from "../lib/time.ts";
import { socketPath, storePath, type Config } from "./config.ts";
import { ProtocolError } from "./protocol.ts";
import { Ring } from "./ring.ts";
import { createServer, type Server } from "./server.ts";
import { hostExtension } from "./extension.ts";
import type { EventStream } from "../durable-worker/harness.ts";
import type { Connection } from "./server.ts";
import { newTurnState, prepareTurn, reminderFor, type StoredEntry, type TurnPort, type Write } from "./turn.ts";

export type { EventStream };
export type Submission = { id: number; status(): Promise<string>; wait(): Promise<"done" | "unanswered"> };
/** `write` appends an entry (`submit({ type: "write", ... })`): the first message and the context updates. */
export type Conversation = { submit(input: Input): Promise<Submission>; write(write: Write): Promise<unknown>; install(extension: Extension): void; entries(): Promise<readonly unknown[]>; closed: Promise<unknown>; watch(): Promise<EventStream>; abort(opts: { background: boolean }): Promise<void>; abortSubmission(id: number): Promise<"aborted" | "already_placed" | "settled" | "not_found"> };

type Model = { provider: string; modelId: string; thinkingLevel?: string };

export type Deps = {
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  resolveModel(pattern: string): Model;
  contextFiles(cwd: string): readonly { path: string; content: string }[];
  open(o: { file: string; cwd: string; model: Model; extension: Extension }): Promise<Conversation>;
  follow?(hooks: FollowerHooks): { start(): void; stop(): void };
  listen(path: string, accept: (conn: Connection) => { receive(line: string): void; closed(): void }): Promise<{ close(): Promise<void> }>;
  newEpoch(): string;
  onTerminate(handler: () => void): void;
  homeDir(): string;
  now(): Date;
  log(line: string): void;
};

/** `conversation.closed` settles with a pi-durable `SessionEnd`: `{ reason: 'closed' } | { reason: 'failed', error }`. */
function endText(end: unknown): string {
  if (typeof end !== "object" || end === null) return String(end);
  const { reason, error } = end as { reason?: unknown; error?: unknown };
  if (reason !== "failed") return String(reason ?? "closed");
  return `failed: ${error instanceof Error ? error.message : String(error)}`;
}

/** The host's own version (the wire's `host_version`) and the pi-durable it is built on (package.json pins 1.3.0). */
const HOST_VERSION = "1";
const PI_DURABLE = "1.3.0";
/** Timers of the server: the global ones, unref-ed so a pending hello timeout never holds the process. */
const timers = {
  after(ms: number, fn: () => void): () => void {
    const t = setTimeout(fn, ms);
    t.unref();
    return () => clearTimeout(t);
  },
};

const contextText = (files: readonly { path: string; content: string }[]): string | undefined =>
  files.length === 0 ? undefined : files.map((f) => `## ${f.path}\n${f.content}`).join("\n\n");

/** Runs one Lead's or Manager's Session (contract §6.13). Resolves only on terminate (143), a closed conversation (1) or a failure (1); a `usage` error is thrown before any effect. */
export async function runHost(config: Config, deps: Deps): Promise<number> {
  // A path over the limit is a usage error before any effect.
  const sock = socketPath(config.home, config.session);
  const model = deps.resolveModel(config.model);
  const failed = (what: string, e: unknown) => deps.log(`${what}: ${e instanceof Error ? e.message : String(e)}`);

  const shown = await deps.cpd(["session", "show", config.session]);
  const session = parseSession(shown);
  if (session.role !== "lead" && session.role !== "manager") throw new CpdError("usage", `session ${config.session} has role ${session.role}: the host runs a lead or a manager only`);

  let follower: { start(): void; stop(): void } | undefined;
  let server: Server | undefined;
  let listener: { close(): Promise<void> } | undefined;
  let current: EventStream | undefined;
  let over = false;
  let byeReason: "shutdown" | "storage_failed" = "shutdown";
  let end!: (code: number, reason?: "shutdown" | "storage_failed") => void;
  const ended = new Promise<number>((resolve) => {
    end = (code, reason = "shutdown") => {
      if (!over) byeReason = reason;
      resolve(code);
    };
  });
  deps.onTerminate(() => end(143));

  const reporter = new Reporter(
    async (w: Wanted) => {
      await deps.cpd(stateArgs(w, config.session));
    },
    (e) => failed("session state", e),
  );

  const start = async (): Promise<void> => {
    const policy = policySection(await deps.cpd(["policy", "show"]));
    const base = hostExtension({ contextFiles: contextText(deps.contextFiles(config.cwd)), policy, zone: zoneLine(deps.now()) }, { cwd: config.cwd, home: deps.homeDir() });
    const turn = newTurnState();
    let port!: TurnPort;
    // pi-durable 1.3.0 (README "Hooks", GenerationHooks): `onYield(answer)` returning `{ continue }` appends a user message and continues the run; the answer is the final AssistantMessage.
    const extension: Extension = {
      ...base,
      hooks: [...(base.hooks ?? []), hook(GenerationTask, { onYield: async (answer) => {
        const text = await reminderFor([answer], port, turn);
        return text === undefined ? undefined : { continue: text };
      } })],
    };
    const conversation = await deps.open({ file: storePath(config.home, config.session), cwd: config.cwd, model, extension });
    current = await conversation.watch();
    conversation.closed.then(
      (reason) => {
        failed("the conversation closed", endText(reason));
        end(1, "storage_failed");
      },
      (e) => {
        failed("the conversation closed", e);
        end(1, "storage_failed");
      },
    );
    port = {
      seat: { id: config.session, role: session.role as "lead" | "manager", context: session.context, plan: session.plan, registeredEvent: session.registeredEvent, byUser: session.byUser },
      cpd: deps.cpd,
      submit: (w) => conversation.write(w),
      entries: async () => (await conversation.entries()) as readonly StoredEntry[],
      log: deps.log,
    };
    let prepared: Promise<void> = Promise.resolve();
    let preparing = 0;
    // §3.6 allows `stopped -> idle`, never `stopped -> working`.
    if ((shown.session as { state?: unknown }).state === "stopped") reporter.want({ state: "idle" });

    let cursor = cursorFrom(session.registeredEvent, await conversation.entries(), config.session);
    let pending = 0;
    let reply = false;
    let hostState: "idle" | "working" = "idle";
    const changed = (next: "idle" | "working"): void => {
      if (hostState === next) return;
      hostState = next;
      server?.stateChanged();
    };
    const seen = new Set<string>();
    const names = new Map<string, Promise<string | null>>();
    const nameOf = (id: string): Promise<string | null> => {
      let name = names.get(id);
      if (name === undefined) {
        name = deps.cpd(["session", "show", id]).then((s) => parseSession(s).name, (e) => {
          failed(`the sender ${id}`, e);
          return null;
        });
        names.set(id, name);
      }
      return name;
    };
    const submit = async (input: Input): Promise<Submission> => {
      // A reject never waits to prepare: it stores nothing when the conversation is busy (the context update then comes with the next turn).
      const starts = pending === 0 && input.whenBusy !== "reject";
      if (input.reply) reply = true;
      pending++;
      reporter.want({ state: "working" });
      changed("working");
      try {
        // Only an input that starts a turn gets the first message and the context update; they are prepared one input at a time.
        if (starts) {
          preparing++;
          prepared = prepared.then(() => prepareTurn(port, turn)).finally(() => preparing--);
        }
        if (preparing > 0) await prepared;
        const submission = await conversation.submit(input);
        void submission.wait().then(
          () => {
            if (--pending === 0) {
              reporter.want(reply ? { state: "idle", reply: true } : { state: "idle" });
              reply = false;
              changed("idle");
            }
          },
          (e) => {
            failed("a submission", e);
            end(1);
          },
        );
        return submission;
      } catch (e) {
        if (--pending === 0) {
          reporter.want({ state: "idle" });
          reply = false;
          changed("idle");
        }
        throw e;
      }
    };

    // The event stream: the ring of what clients are caught up on (contract §6.14).
    const first = current!;
    const epoch = deps.newEpoch();
    const ring = new Ring(epoch, first.snapshot);
    let from = "";
    let nextClient = 0;
    const srv = createServer({
      session: config.session,
      role: session.role as "lead" | "manager",
      cwd: config.cwd,
      hostVersion: HOST_VERSION,
      piDurable: PI_DURABLE,
      epoch,
      ring,
      submit: async ({ text, whenBusy, requestId }) => {
        const id = requestId ?? randomUUID();
        try {
          const s = await submit({ content: text, requestId: `chat:${id}`, whenBusy, entry: { kind: "cpd-chat", data: { client_id: from, request_id: id } }, reply: true });
          return { submissionId: s.id, status: await s.status() };
        } catch (e) {
          if (e instanceof ConversationBusy) throw new ProtocolError("busy", e.message);
          throw e;
        }
      },
      abort: (opts) => conversation.abort(opts),
      abortSubmission: (id) => conversation.abortSubmission(id),
      hostState: () => ({ state: hostState, ...(session.name === null ? {} : { name: session.name }) }),
      timers,
      clientId: () => (from = `c${++nextClient}`),
    });
    server = srv;
    const attachStream = (s: EventStream): void => {
      s.start(async (events) => {
        if (s !== current) return;
        srv.frame(ring.append([...events]));
        if (!ring.shouldRebase(events.some((e) => (e as { type?: unknown }).type === "run_end"))) return;
        // Retire this stream first (events it delivers after this point are in the next snapshot); its stop is not awaited, since it may wait for this very listener (unverified in pi-durable 1.3.0).
        current = undefined;
        s.stop().catch((e) => failed("stopping the stream", e));
        const next = await conversation.watch();
        current = next;
        srv.frame(ring.rebase(next.snapshot));
        attachStream(next);
      });
      s.closed.then((reason) => {
        if (s !== current) return;
        const why = (reason as { reason?: unknown } | null)?.reason;
        if (why === "stopped") return;
        failed("the event stream ended", endText(reason));
        end(1, "storage_failed");
      });
    };
    attachStream(first);
    listener = await deps.listen(sock, (conn) => {
      const h = srv.connect(conn);
      let client = "";
      return {
        receive: (line) => {
          // `ServerDeps.submit` carries no client id: the id the server gave this connection is read here, synchronously, while it calls us.
          from = client;
          h.receive(line);
          client = from;
        },
        closed: () => h.closed(),
      };
    });
    if (over) await listener.close();
    follower = deps.follow?.({
      before: async () => {},
      onRunStart: () => {},
      onLine: (text) => {
        const line = parseLine(text);
        if (line.type !== "event") {
          failed("follower line", line.type === "failure" ? line.error : new CpdError("bad_output", line.type));
          return false;
        }
        const event = line.event;
        cursor = Math.max(cursor, event.id);
        if (seen.has(`event:${event.id}`)) return true;
        seen.add(`event:${event.id}`);
        const fail = (e: unknown) => {
          failed("an event", e);
          end(1);
        };
        // Submitted synchronously unless a sender's name must be read first: a lookup is the only thing that waits.
        const from = event.kind === "session-message" && event.session === config.session ? event.data.from : null;
        if (typeof from === "string") {
          nameOf(from)
            .then((name) => {
              const input = inputOf(event, config.session, session, name);
              return input && submit(input);
            })
            .catch(fail);
        } else {
          const input = inputOf(event, config.session, session);
          if (input) submit(input).catch(fail);
        }
        return true;
      },
      onFailure: (e) => failed("follower", e),
      cursor: () => cursor,
    });
    follower?.start();
  };
  start().catch((e) => {
    failed("the run", e);
    end(1);
  });

  const code = await ended;
  over = true;
  follower?.stop();
  server?.shutdown(byeReason);
  await listener?.close().catch((e) => failed("closing the listener", e));
  await current?.stop().catch((e) => failed("stopping the stream", e));
  await reporter.close({ state: "stopped" });
  return code;
}
