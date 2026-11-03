import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ConversationBusy, GenerationTask, hook, section, type Extension } from "@earendil-works/pi-durable";
import { createCodemodeExtension } from "@earendil-works/pi-durable/codemode";
import { CodemodeSandbox } from "@earendil-works/pi-codemode";
import type { FollowerHooks } from "../lib/follower.ts";
import { parseLine, type EventOf } from "../lib/event.ts";
import { cursorFrom } from "./cursor.ts";
import { inputOf, type Input } from "./deliver.ts";
import { CpdError, toCpdError } from "../lib/cpd.ts";
import { adapterPath } from "../lib/reload.ts";
import { skillOf } from "../lib/skills.ts";
import { codemodeNamespaces } from "./codemode.ts";
import { createReload, type ReloadPort } from "./reload.ts";
import { createRetier } from "./retier.ts";
import { skillsCheck, skillsSection, type SkillsPort } from "./skills.ts";
import { highestReview, reviewsFromEntries } from "../lib/plan-review.ts";
import type { Host } from "../lib/host.ts";
import { clearTrip, ENTRY_TYPE, holdTrip, LABEL, trippedIn, WINDOW_MS } from "../lib/loop-guard.ts";
import { policySection } from "../lib/policy.ts";
import { parseSession, type Run } from "../lib/run.ts";
import { Reporter, stateArgs, type Wanted } from "../lib/state.ts";
import { zoneLine } from "../lib/time.ts";
import { socketPath, storePath, type Config } from "./config.ts";
import * as held from "./held.ts";
import { createDialogs } from "./dialogs.ts";
import { createPlanReview } from "./plan-review.ts";
import { HISTORY_LIMIT, LIMITS, ProtocolError, type AgentState } from "./protocol.ts";
import { HostRunning } from "./sockets.ts";
import { Ring } from "./ring.ts";
import { createServer, type Server } from "./server.ts";
import { hostExtension } from "./extension.ts";
import { registerHostTools } from "./tools.ts";
import type { ViewCall, ViewStream } from "../durable-worker/harness.ts";
import type { Connection } from "./server.ts";
import { newTurnState, prepareTurn, reminderFor, type StoredEntry, type TurnPort, type Write } from "./turn.ts";

export type { ViewCall, ViewStream };
export type Submission = { id: number; status(): Promise<string>; wait(): Promise<"done" | "unanswered"> };
/** `write` appends an entry (`submit({ type: "write", ... })`): the first message and the context updates. */
/** The change a retier applies (contract §6.18): pi-durable's `AgentChange`, narrowed to the two fields the host sets. */
export type AgentChange = { model?: { provider: string; modelId: string } | null; thinkingLevel?: string | null };
export type Conversation = { submit(input: Input): Promise<Submission>; write(write: Write): Promise<unknown>; install(extension: Extension): void; entries(): Promise<readonly unknown[]>; closed: Promise<unknown>; watch(): Promise<ViewStream>; abort(opts: { background: boolean }): Promise<void>; abortSubmission(id: number): Promise<"aborted" | "already_placed" | "settled" | "not_found"> } & ConversationCommands;

/** The four conversation commands of contract 6.17 a `Conversation` must carry (built by `conversationOf`). */
export type ConversationCommands = {
  compact(instructions?: string): Promise<number>;
  configure(change: AgentChange): Promise<void>;
  agent(): Promise<{ model?: { provider: string; modelId: string }; thinkingLevel: string }>;
  pageEntries(limit: number, cursor?: object): Promise<{ entries: readonly unknown[]; next?: object }>;
};

type Model = { provider: string; modelId: string; thinkingLevel?: string };

/** The loop guard's runtime (contract §6.16): the bound `cpd session show` gives, the window of self-started turns the host keeps, and the trip. */
export type LoopRuntime = {
  role: "lead" | "manager";
  info: { loopGuardTurns: number };
  turnStarts: number[];
  tripped: boolean;
};

/**
 * The lib's window rule (`registerLoopGuard`, loop-guard.ts), pure: folds the self-started turn at `now` into `r.turnStarts`
 * and returns whether the window holds more than `r.info.loopGuardTurns`.
 */
export function loopGate(r: LoopRuntime, now: number): boolean {
  r.turnStarts = [...r.turnStarts.filter((t) => t > now - WINDOW_MS), now];
  return r.turnStarts.length > r.info.loopGuardTurns;
}

/** One model of `list_models` and of the `configure` validation (contract §6.17). */
export type ModelInfo = { provider: string; model_id: string; name: string; thinking_levels: readonly string[]; context_window: number };

/** pi-durable's `Agent` narrowed to the two fields the wire's `AgentState` carries. */
const agentOf = (a: { model?: { provider: string; modelId: string }; thinkingLevel: string }): AgentState => ({
  ...(a.model === undefined ? {} : { model: { provider: a.model.provider, model_id: a.model.modelId } }),
  thinking_level: a.thinkingLevel,
});

export type Deps = {
  /** The one-owner check: rejects with `HostRunning` when a host already owns the socket. */
  guard(path: string): Promise<void>;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  resolveModel(pattern: string): Model;
  contextFiles(cwd: string): readonly { path: string; content: string }[];
  open(o: { file: string; cwd: string; model: Model; extension: Extension }): Promise<Conversation>;
  /** pi/lib/reload.ts's `validatePackage` over the adapter package directory: the refusal reason, or `undefined`. */
  validatePackage(dir: string): Promise<string | undefined>;
  follow?(hooks: FollowerHooks): { start(): void; stop(): void };
  /** The one source of truth of `list_models` and of the `configure` validation (contract §6.17). */
  models(): Promise<readonly ModelInfo[]>;
  listen(path: string, accept: (conn: Connection) => { receive(line: string): void; closed(): void }): Promise<{ close(): Promise<void> }>;
  newEpoch(): string;
  onTerminate(handler: () => void): void;
  homeDir(): string;
  now(): Date;
  log(line: string): void;
};

/** A `WatchEnd` (`{ kind: 'stopped' | 'aborted' | 'ended' } | { kind: 'listener_error' | 'failed', error }`) or a `SessionEnd` (`{ reason }`). */
function endText(end: unknown): string {
  if (typeof end !== "object" || end === null) return String(end);
  const { kind, reason, error } = end as { kind?: unknown; reason?: unknown; error?: unknown };
  if (kind !== undefined && kind !== "failed" && kind !== "listener_error") return String(kind);
  if (kind === undefined && reason !== "failed") return String(reason ?? "closed");
  return `failed: ${error instanceof Error ? error.message : String(error)}`;
}

/** The host's own version (the wire's `host_version`) and the pi-durable it is built on (package.json pins 1.5.0). */
const HOST_VERSION = "1";
export const PI_DURABLE = "1.5.0";
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
  // The one-owner check is the first thing the host does, before it reads the Session, resolves the model, opens the store or reports any state (contract §6.13).
  // A host that finds another host running logs `host_running` and returns 1 with no effect on the Session.
  const sock = socketPath(config.home, config.session);
  try {
    await deps.guard(sock);
  } catch (e) {
    if (e instanceof HostRunning) {
      deps.log(e.message);
      return 1;
    }
    // Any other guard failure is thrown unchanged, before any effect (fail loud).
    throw e;
  }
  const model = deps.resolveModel(config.model);
  const failed = (what: string, e: unknown) => deps.log(`${what}: ${e instanceof Error ? e.message : String(e)}`);

  const shown = await deps.cpd(["session", "show", config.session]);
  const session = parseSession(shown);
  if (session.role !== "lead" && session.role !== "manager") throw new CpdError("usage", `session ${config.session} has role ${session.role}: the host runs a lead or a manager only`);

  let follower: { start(): void; stop(): void } | undefined;
  let server: Server | undefined;
  let listener: { close(): Promise<void> } | undefined;
  let current: ViewStream | undefined;
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
    const turn = newTurnState();
    const plan = session.plan ?? "";
    // §6.16 the loop guard: the runtime the lib's `holdTrip`/`clearTrip`/`trippedIn` read. `facts` satisfies the lib (the
    // blocked state is reported from `wanted()` by `loop.tripped`), and `loopHost` carries the effects the lib calls back into.
    const loop: LoopRuntime & { facts: { open(key: string, label: string, kind: string): void; close(key: string): void } } = {
      role: session.role as "lead" | "manager",
      info: { loopGuardTurns: session.loopGuardTurns },
      turnStarts: [],
      tripped: false,
      facts: { open: () => {}, close: () => {} },
    };
    // §6.16 held deliveries: while the breaker is tripped an input that would start a turn is queued here, not submitted.
    const heldLoop: Input[] = [];
    // §6.15 "The blocked state": a held request reports `blocked` with the label `dialog`; resolving to zero returns to
    // `working` while a run is pending, else to `idle`, with the reply rule of §6.13 untouched. A loop-guard trip reports
    // `blocked` with the label `φ: loop guard` by label, not by a held request (§6.16).
    let hostState: "idle" | "working" = "idle";
    let reply = false;
    const wanted = (): Wanted =>
      loop.tripped ? { state: "blocked", label: LABEL } : held.heldCount() > 0 ? { state: "blocked", label: "dialog" } : hostState === "working" ? { state: "working" } : reply ? { state: "idle", reply: true } : { state: "idle" };
    const report = (): void => {
      server?.stateChanged();
      reporter.want(wanted());
    };
    // The adapter the lib calls: the pi entry (a silent conversation write) and the refresh.
    const loopHost = {
      pi: { appendEntry: (type: string, data: unknown) => void conversation.write({ type: "write", entry: { kind: type, data: data as Record<string, unknown>, model: [] } }).catch((e) => failed("writing a loop-guard entry", e)) },
      refresh: () => report(),
    } as unknown as Host;
    const changed = (next: "idle" | "working"): void => {
      if (hostState === next) return;
      hostState = next;
      report();
    };
    // The dialogs broadcast through the server and re-report the state on every request that appears or resolves.
    const dialogs = createDialogs({
      held,
      broadcast: (msg) => server?.broadcast(msg),
      stateChanged: () => {
        server?.stateChanged();
        reporter.want(wanted());
      },
    });
    let port!: TurnPort;
    // The four tools of §6.15 ride on the extension `deps.open` installs (the extension's `tools` carry them).
    const hostTools = registerHostTools({
      role: session.role as "lead" | "manager",
      plan: session.plan ?? undefined,
      name: session.name ?? "",
      cpd: deps.cpd,
      held,
      dialogs,
    });
    // §6.18 code mode: the QuickJS sandbox and one namespace per attached MCP server. No MCP attach is wired in P1.9 (out of scope), so the list is empty today.
    const sandbox = new CodemodeSandbox();
    const skill = skillOf(session.role);
    const skillDir = skill === undefined ? undefined : join(adapterPath(), "skills", skill);
    const codemodeServers: readonly { server: string; tools: readonly string[] }[] = [];
    // The build the start and a reload share: re-reads the prompt sources and the role skill, and rebuilds the extension modules.
    const build = async (): Promise<Extension> => {
      const policy = policySection(await deps.cpd(["policy", "show"]));
      const base = hostExtension({ contextFiles: contextText(deps.contextFiles(config.cwd)), policy, zone: zoneLine(deps.now()) }, { cwd: config.cwd, home: deps.homeDir(), visible: [] }, session.role as "lead" | "manager");
      const sections = [...(base.sections ?? [])];
      if (skill !== undefined && skillDir !== undefined) sections.push(section("cpd-skills", () => skillsSection(skill, skillDir)));
      const codemode = createCodemodeExtension({ sandbox, namespaces: codemodeNamespaces(codemodeServers) });
      // pi-durable 1.3.0 (README "Hooks", GenerationHooks): `onYield(answer)` returning `{ continue }` appends a user message and continues the run; the answer is the final AssistantMessage.
      return {
        ...base,
        sections: [...sections, ...(codemode.sections ?? [])],
        hooks: [...(base.hooks ?? []), hook(GenerationTask, { onYield: async (answer) => {
          const text = await reminderFor([answer], port, turn);
          return text === undefined ? undefined : { continue: text };
        } })],
        tools: [...(hostTools.tools ?? []), ...(codemode.tools ?? [])],
      };
    };
    const extension = await build();
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

    // The plan-review flow (§6.15): its summary and feedback files live in `$CPD_HOME/run/<plan>/` (review.rs `summary_review_md`).
    const readPlanFile = async (file: string): Promise<string> => {
      const path = join(config.home, "run", plan, file);
      try {
        return await readFile(path, "utf8");
      } catch (e) {
        throw new CpdError("file_unreadable", `cannot read ${path}: ${e instanceof Error ? e.message : String(e)}`);
      }
    };
    const planReview = createPlanReview({
      plan,
      cpd: deps.cpd,
      readText: readPlanFile,
      write: (entry) => conversation.write({ type: "write", entry }),
      entries: () => conversation.entries(),
      dialogs,
      held,
    });

    const storedEntries = await conversation.entries();
    let cursor = cursorFrom(session.registeredEvent, storedEntries, config.session);
    // L7: the last persisted `cpd-loop-guard` entry says whether the breaker is tripped; the durable entry is mapped to the
    // pi session shape `trippedIn` reads (§6.16 unverified: `{ kind, data }` is not `{ type, customType, data }`).
    loop.tripped = trippedIn(storedEntries.map((e) => {
      const record = e as { kind?: unknown; data?: unknown };
      return { type: "custom", customType: record.kind, data: record.data };
    }));
    // A restart that begins tripped reports `blocked` at once, so `cpd session resume` (which needs that state) can clear it (L7).
    if (loop.tripped) report();
    let pending = 0;
    // §6.18 skills: the check at start and after every reload, over the conversation's entries.
    const skillsPort: SkillsPort = {
      entries: async () => (await conversation.entries()) as readonly { kind: string; data?: Record<string, unknown> }[],
      write: (entry) => conversation.write({ type: "write", entry }),
      broadcast: (msg) => server?.broadcast(msg),
      deliver: (error) => failed(error.what, new CpdError(error.code, error.message, error.details)),
      log: deps.log,
    };
    const runSkillsCheck = async (): Promise<void> => {
      if (skill === undefined || skillDir === undefined) return;
      try {
        await skillsCheck(skillsPort, skill, skillDir);
      } catch (e) {
        failed("skills check", e);
      }
    };
    // §6.18 reload: the guard from the host's own state, the shared build, and `deps.validatePackage` over the adapter path.
    const reloadPort: ReloadPort = {
      rebuild: () => build(),
      guard: () => {
        const state = wanted().state;
        return { busy: pending > 0, pending: false, state: state === "stopped" ? "idle" : state };
      },
      validate: async () => {
        const reason = await deps.validatePackage(adapterPath());
        return reason === undefined ? { ok: true } : { ok: false, reason };
      },
      install: async (installed) => {
        conversation.install(installed as Extension);
        await runSkillsCheck();
      },
    };
    const reload = createReload(reloadPort);
    // §6.18 retier: apply the `session-retiered` agent change at idle, in one commit, and report back to the engine.
    const retier = createRetier({
      idle: () => pending === 0 && hostState === "idle" && held.heldCount() === 0,
      configure: (change) => conversation.configure(change as AgentChange),
      report: async (kind, detail) => {
        await deps.cpd(kind === "switched" ? ["session", "switched", "--session", config.session] : ["session", "switch-failed", "--code", "model_unavailable", "--message", detail ?? "not idle", "--session", config.session]);
      },
    });
    // The cached `AgentState` of contract §6.17: read back after each `configure`; absent until the first one (a `host_state` carrying no agent).
    let agentState: AgentState | undefined;
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
      // §6.16 the loop gate: every input that reaches the conversation — a client `submit` or a follower Event — is a
      // submission the host makes, so it counts here, at the one gate. The User's own `session-message` (`inputOf` sets
      // `reply: true` for it) does not count: it empties the window and clears a trip, as the adapter's `messageType === "user"` does.
      const selfUser = input.reply === true && input.entry.kind === "cpd-event" && input.entry.data.kind === "session-message";
      if (selfUser) {
        if (clearTrip(loopHost, loop as unknown as Run)) report();
      } else if (!loop.tripped) {
        const at = deps.now().getTime();
        if (loopGate(loop, at)) {
          holdTrip(loop as unknown as Run);
          conversation
            .write({ type: "write", entry: { kind: ENTRY_TYPE, data: { tripped: true, at, turns: loop.turnStarts.length }, model: [] } })
            .catch((e) => failed("writing the loop-guard entry", e));
          report();
        }
      }
      // A reject never waits to prepare: it stores nothing when the conversation is busy (the context update then comes with the next turn).
      const starts = pending === 0 && input.whenBusy !== "reject";
      // §6.16 held deliveries: while tripped an input that would start a turn is queued, not submitted (a `reject` is still refused).
      if (loop.tripped && starts) {
        heldLoop.push(input);
        return { id: -1, status: async () => "held", wait: async () => "unanswered" };
      }
      if (input.reply) reply = true;
      pending++;
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
              changed("idle");
              reply = false;
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
          reply = false;
          changed("idle");
        }
        throw e;
      }
    };

    // §6.13 step 7 (hs-n658/hs-d812): a fresh durable Lead/Manager is not by-user and has no mapped Events, so it
    // has no input of its own at birth; the engine's launch passes no pi argv `@<lead-message.md>` either. The host
    // reads the Lead's first message and runs it as the first turn's input, counted by the cursor. A Session whose
    // conversation already holds its `cpd-first-message` entry (a restart) submits nothing and reads nothing.
    if (!session.byUser && !storedEntries.some((e) => { const entry = e as StoredEntry; return entry.kind === "cpd-first-message" && entry.data?.delivered_to === config.session; })) {
      try {
        const out = await deps.cpd(["session", "first-message", "--session", config.session]);
        if (typeof out.text !== "string" || !Number.isSafeInteger(out.event)) throw new Error("cpd session first-message: the output lacks text or event");
        const event = out.event as number;
        // The submit below is turn-starting: mark the first message done and its Event the cursor, so prepareTurn does
        // not ALSO store it as a silent write and the context update counts it (turn.ts).
        turn.firstDone = true;
        turn.firstEvent = Math.max(turn.firstEvent, event);
        await submit({
          content: out.text,
          requestId: `first-message:${config.session}`,
          whenBusy: "followUp",
          entry: { kind: "cpd-first-message", data: { event, delivered_to: config.session } },
        });
      } catch (e) {
        failed("the first message at startup", e);
      }
    }

    // The event stream: the ring of what clients are caught up on (contract §6.14).
    const first = current!;
    const epoch = deps.newEpoch();
    const ring = new Ring(epoch, first.value);
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
      submit: async ({ text, whenBusy, requestId, images }) => {
        const id = requestId ?? randomUUID();
        const content =
          images === undefined || images.length === 0
            ? text
            : [{ type: "text" as const, text }, ...images.map((i) => ({ type: "image" as const, data: i.data, mimeType: i.mimeType }))];
        try {
          const s = await submit({ content, requestId: `chat:${id}`, whenBusy, entry: { kind: "cpd-chat", data: { client_id: from, request_id: id } }, reply: true });
          return { submissionId: s.id, status: await s.status() };
        } catch (e) {
          if (e instanceof ConversationBusy) throw new ProtocolError("busy", e.message);
          throw e;
        }
      },
      // Contract §6.17: the five conversation commands.
      compact: async (instructions) => ({ task_id: await conversation.compact(instructions) }),
      configure: async (change) => {
        if (pending > 0) throw new ProtocolError("refused", "a run is pending");
        // The current agent is read back before the first configure, so a change that names only a thinking level validates against it.
        if (agentState === undefined) agentState = agentOf(await conversation.agent());
        const list = await deps.models();
        let ref: { provider: string; modelId: string } | undefined;
        if (change.model !== undefined) {
          const i = change.model.indexOf("/");
          const provider = i < 0 ? "" : change.model.slice(0, i);
          const modelId = i < 0 ? change.model : change.model.slice(i + 1);
          const found = list.find((m) => m.provider === provider && m.model_id === modelId);
          if (found === undefined) throw new ProtocolError("bad_args", `model must name an offered model: ${change.model}`);
          ref = { provider: found.provider, modelId: found.model_id };
        }
        if (typeof change.thinkingLevel === "string") {
          const current = agentState?.model;
          const target = ref ?? (current === undefined ? undefined : { provider: current.provider, modelId: current.model_id });
          const found = target === undefined ? undefined : list.find((m) => m.provider === target.provider && m.model_id === target.modelId);
          if (found === undefined || !found.thinking_levels.includes(change.thinkingLevel)) throw new ProtocolError("bad_args", `thinking_level must be one of the levels of the model: ${change.thinkingLevel}`);
        }
        await conversation.configure({ ...(ref === undefined ? {} : { model: ref }), ...(change.thinkingLevel === undefined ? {} : { thinkingLevel: change.thinkingLevel }) } as never);
        agentState = agentOf(await conversation.agent());
        return { agent: agentState };
      },
      models: async () => deps.models(),
      fork: async (at, ownership) => {
        if (ownership !== undefined) throw new ProtocolError("bad_args", "ownership must be lead or manager");
        const entries = await conversation.entries();
        if (!entries.some((e) => (e as { id?: unknown }).id === at)) throw new ProtocolError("bad_args", "at must name an entry of this conversation");
        let answered: Record<string, unknown>;
        try {
          answered = await deps.cpd(["session", "fork", config.session]);
        } catch (e) {
          const c = toCpdError(e);
          throw new ProtocolError("refused", c.message, { code: c.code });
        }
        const s = answered.session;
        if (typeof s !== "object" || s === null || typeof (s as { id?: unknown }).id !== "string") throw new ProtocolError("internal", "cpd session fork answered without a session id");
        return { conversation_id: (s as { id: string }).id };
      },
      history: async (before, limit) => {
        let cursor: object | undefined;
        if (before !== undefined) {
          let decoded: unknown;
          try {
            decoded = JSON.parse(Buffer.from(before, "base64").toString("utf8"));
          } catch {
            throw new ProtocolError("bad_args", "before must be the base64 of a page cursor");
          }
          if (typeof decoded !== "object" || decoded === null || Array.isArray(decoded)) throw new ProtocolError("bad_args", "before must be the base64 of a page cursor");
          cursor = decoded as object;
        }
        const nextOf = (c: object): string => Buffer.from(JSON.stringify(c), "utf8").toString("base64");
        const encoded = (p: { entries: readonly unknown[]; next?: object }): string => JSON.stringify({ entries: p.entries, ...(p.next === undefined ? {} : { next: nextOf(p.next) }) });
        let n = limit ?? HISTORY_LIMIT;
        let page = await conversation.pageEntries(n, cursor);
        while (n > 1 && Buffer.byteLength(encoded(page), "utf8") > LIMITS.hostLine) {
          n = Math.max(1, Math.floor(n / 2));
          page = await conversation.pageEntries(n, cursor);
        }
        return { entries: page.entries, ...(page.next === undefined ? {} : { next: nextOf(page.next) }) };
      },
      abort: (opts) => {
        // §6.15 "Abort (4.6.4)": every held request is resolved `aborted` by the host before the conversation abort, one `ui_resolved` each.
        const ids = held.heldList().map((h) => h.id);
        for (const id of ids) {
          held.resolveByHost(id, "aborted");
          server?.broadcast({ type: "ui_resolved", id, outcome: "aborted", by: "host" });
        }
        return conversation.abort(opts);
      },
      abortSubmission: (id) => conversation.abortSubmission(id),
      reload: () => reload.reload(),
      held: () => held.heldList(),
      answerUi: async (id, value) => held.answer(id, value),
      hostState: () => {
        const heldUi = held.heldCount();
        return { state: heldUi > 0 ? "blocked" : hostState, held_ui: heldUi, ...(session.name === null ? {} : { name: session.name }), ...(agentState === undefined ? {} : { agent: agentState }) };
      },
      timers,
      clientId: () => (from = `c${++nextClient}`),
    });
    server = srv;
    const hasRun = (view: unknown): boolean => {
      const live = (view as { docs?: Record<string, { run?: unknown } | undefined> } | undefined)?.docs?.["pi.live"];
      return live?.run !== undefined && live.run !== null;
    };
    let lastView: unknown = first.value;
    const attachStream = (s: ViewStream): void => {
      s.start(async (call: ViewCall) => {
        if (call.kind === "failed") {
          failed("the watch", call.error);
          end(1, "storage_failed");
          return;
        }
        srv.frame(ring.append([...call.ops]));
        const runEnd = hasRun(lastView) && !hasRun(call.value);
        lastView = call.value;
        if (ring.shouldRebase(runEnd)) srv.frame(ring.rebase(call.value));
      });
      // pi-durable 1.5.0 (chord watch.d.ts): `stop()` idempotently stops future callbacks and settles `closed` with `{ kind: 'stopped' }`; any other kind is the watch ending on its own.
      s.closed.then(
        (e) => {
          if (over || (e as { kind?: unknown } | undefined)?.kind === "stopped") return;
          failed("the watch ended", endText(e));
          end(1, "storage_failed");
        },
        (e) => {
          if (over) return;
          failed("the watch ended", e);
          end(1, "storage_failed");
        },
      );
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
    // §6.15 "The start hook": at host start, reopen the dialog of the highest pending review with a fresh id (4.4 rule 6).
    try {
      const { reviews } = reviewsFromEntries(await conversation.entries(), plan);
      const pending = [...reviews.values()].filter((e) => e.status === "pending").map((e) => e.review);
      const highest = highestReview(pending);
      if (highest !== undefined) await planReview.open(highest, "start");
    } catch (e) {
      failed("the plan review at start", e);
    }
    // §6.18: the skills check runs at start, after the host has its conversation.
    await runSkillsCheck();
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
        // A `session-retiered` Event (§6.18): resolve the agent change from `cpd session show` and apply it at idle.
        if (event.kind === "session-retiered") {
          void (async () => {
            try {
              const answer = await deps.cpd(["session", "show", config.session]);
              const s = answer.session as { model?: unknown; thinking?: unknown } | undefined;
              if (typeof s?.model !== "string" || typeof s?.thinking !== "string") throw new CpdError("bad_output", "cpd session show: the session object lacks model or thinking");
              const slash = s.model.indexOf("/");
              await retier.apply({ model: slash > 0 ? { provider: s.model.slice(0, slash), modelId: s.model.slice(slash + 1) } : null, thinkingLevel: s.thinking });
            } catch (e) {
              failed("a retier event", e);
            }
          })();
          return true;
        }
        // A `plan-review` Event feeds the flow of §plan-review.ts instead of a submission: pending opens the dialog,
        // approved or rejected settles the pending entry, superseded closes it (pi/lib/handle.ts `handleEvent`).
        if (event.kind === "plan-review") {
          const ev = event as EventOf<"plan-review">;
          void (async () => {
            try {
              const review = ev.data.review;
              if (ev.data.status === "pending") {
                await planReview.open(review, "event");
                return;
              }
              const { reviews } = reviewsFromEntries(await conversation.entries(), plan);
              const entry = reviews.get(review);
              if (entry?.status !== "pending") return;
              if (ev.data.status === "approved" || ev.data.status === "rejected") await planReview.decidedElsewhere(entry, ev);
              else if (ev.data.status === "superseded") planReview.closeSuperseded(entry);
            } catch (e) {
              failed("a plan-review event", e);
            }
          })();
          return true;
        }
        // §6.16 the clear: a `session-resumed` Event for this Session while tripped calls the lib's `clearTrip`; then the held
        // deliveries are submitted (one turn that sees what queued). A `session-resumed` when not tripped changes nothing.
        if (event.kind === "session-resumed") {
          if (event.session === config.session) {
            void (async () => {
              try {
                if (!clearTrip(loopHost, loop as unknown as Run)) return;
                for (const queued of heldLoop.splice(0)) await submit(queued);
              } catch (e) {
                failed("a session-resumed event", e);
              }
            })();
          }
          return true;
        }
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
