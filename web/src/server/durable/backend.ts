import {
  IMAGE_MAX_COUNT,
  IMAGE_MAX_TOTAL_BASE64,
  WINDOW_SIZE,
  WebError,
  toWireError,
  type Capabilities,
  type ConfigureChange,
  type Cursor,
  type Dialog,
  type DialogResponse,
  type FooterStats,
  type ModelChoice,
  type QueuedSubmission,
  type SessionRef,
  type Stat,
  type WhenBusy,
  type Window,
  type WireImage,
} from "../../protocol/domain.ts";
import { sessionRetierArgs } from "../cockpit/actions.ts";
import { createDialect, dialogFromRequest } from "./frames.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import type { HostClient, HostFrame } from "./client.ts";

// local copy of CONTRACT 6.1/6.3; replaced by imports in the integration cleanup
export interface Cpd {
  json(args: readonly string[]): Promise<Readonly<Record<string, unknown>>>;
}
export interface GitBoundary {
  branch(cwd: string): Promise<string | null>;
}
export interface ResolvedSession {
  readonly ref: SessionRef;
  readonly file?: string;
  readonly socketPath?: string;
  readonly tierOptions: readonly string[];
}
export type SessionEmit = (message: SessionServerMessage) => void;
export interface SessionHandle {
  readonly capabilities: Capabilities;
  loadEarlier(before: Cursor): Promise<void>;
  submit(input: { clientSubmissionId: string; text: string; images: WireImage[]; whenBusy: WhenBusy }): Promise<void>;
  abort(): Promise<void>;
  abortSubmission(clientSubmissionId: string): Promise<void>;
  compact(instructions?: string): Promise<void>;
  configure(change: ConfigureChange): Promise<void>;
  listModels(): Promise<void>;
  respond(dialogId: string, response: DialogResponse): Promise<void>;
  dispose(): Promise<void>;
}
export interface SessionBackend {
  attach(session: ResolvedSession, emit: SessionEmit): Promise<SessionHandle>;
}

const FIRST_SNAPSHOT_MS = 5000; // same bound as the hello handshake (LIMITS.helloMs)
const known = <T>(value: T): Stat<T> => ({ state: "known", value });
const unknown = <T>(reason: string): Stat<T> => ({ state: "unknown", reason });
const yes = { supported: true } as const;

function capabilitiesOf(tierOptions: readonly string[]): Capabilities {
  return {
    images: { supported: true, maxCount: IMAGE_MAX_COUNT, maxTotalBase64: IMAGE_MAX_TOTAL_BASE64 },
    steer: yes,
    followUp: yes,
    abort: yes,
    abortSubmission: yes,
    compact: yes,
    configure: { model: yes, thinking: yes, tier: { supported: true, options: tierOptions } },
    dialogs: { mode: "interactive" },
  };
}

type HostModel = Awaited<ReturnType<HostClient["listModels"]>>[number];
type PendingDialog = { dialog: Dialog };

export function createDurableBackend(deps: {
  connect: (socketPath: string) => Promise<HostClient>;
  cpd: Cpd;
  git: GitBoundary;
  scheduler: Scheduler;
  windowSize?: number;
}): SessionBackend {
  const windowSize = deps.windowSize ?? WINDOW_SIZE;

  return {
    async attach(session, emit) {
      if (session.socketPath === undefined) {
        throw new WebError("host_unreachable", `durable session ${session.ref.id} has no socket path`, { reason: "no_socket_path" });
      }
      const sessionId = session.ref.id;
      const client = await deps.connect(session.socketPath);
      const capabilities = capabilitiesOf(session.tierOptions);
      const dialect = createDialect();
      const subscriptions: Disposable[] = [];

      let disposed = false;
      let gone: WebError | null = null;
      let ready = false;
      let busy = false;
      let tier = session.ref.tier;
      let agentModel: string | undefined;
      let agentThinking: string | null | undefined;
      let branch: Stat<string> = unknown("branch not read yet");
      let models: HostModel[] | null = null;
      let modelsReason = "model list not loaded";
      let queued: QueuedSubmission[] = [];
      let pendingFeedback: string | null = null;
      let lastFooter = "";
      let lastStatus = "";
      const dialogs = new Map<string, PendingDialog>();
      const early: HostFrame[] = [];

      const send: SessionEmit = (message) => {
        if (!disposed) emit(message);
      };
      const fail = (error: unknown): void => {
        send({ type: "session.error", sessionId, error: toWireError(error) });
      };

      const footer = (): FooterStats => {
        const facts = dialect.transcript.facts;
        const model = agentModel ?? facts.model;
        const thinking = agentThinking === undefined ? facts.thinking : agentThinking;
        const window = model === undefined ? undefined : models?.find((m) => `${m.provider}/${m.model_id}` === model)?.context_window;
        return {
          model: model === undefined ? unknown("the host reported no model") : known(model),
          tier: tier === undefined ? unknown("the session has no tier") : known(tier),
          thinking: typeof thinking === "string" ? known(thinking) : unknown("no thinking level set"),
          contextPercent:
            facts.lastContextTokens === undefined
              ? unknown("no assistant usage in the transcript yet")
              : window === undefined
                ? unknown(models === null ? modelsReason : "the context window of the current model is unknown")
                : known(Math.round((facts.lastContextTokens / window) * 100)),
          cost: known(facts.costUsd),
          branch,
        };
      };
      const emitFooter = (force: boolean): FooterStats => {
        const f = footer();
        const key = JSON.stringify(f);
        if (force || key !== lastFooter) {
          lastFooter = key;
          send({ type: "session.footer", sessionId, footer: f });
        }
        return f;
      };
      const emitStatus = (force: boolean): void => {
        const key = JSON.stringify({ busy, queued });
        if (force || key !== lastStatus) {
          lastStatus = key;
          send({ type: "session.status", sessionId, busy, queued: [...queued] });
        }
      };
      const tailWindow = (): Window => {
        const items = dialect.transcript.items;
        const start = Math.max(0, items.length - windowSize);
        return { items: items.slice(start), hasEarlier: start > 0, cursor: start > 0 ? `dur:${start}` : null };
      };

      const onDialogRequest = (frame: Extract<HostFrame, { type: "ui_request" }>): void => {
        const refuse = (): void => {
          client.respond(frame.id, { cancelled: true }).catch(fail);
        };
        let result: ReturnType<typeof dialogFromRequest>;
        try {
          result = dialogFromRequest(frame, dialect.transcript.items);
        } catch (e) {
          fail(e);
          refuse();
          return;
        }
        if ("kind" in result && result.kind === "editor") {
          if (pendingFeedback === null) {
            refuse();
          } else {
            const value = pendingFeedback;
            pendingFeedback = null;
            client.respond(frame.id, { value }).catch(fail);
          }
          return;
        }
        if ("kind" in result) {
          fail(new WebError("protocol_error", `unsupported dialog ${result.method}/${result.purpose}`));
          refuse();
          return;
        }
        dialogs.set(result.id, { dialog: result });
        send({ type: "session.dialog", sessionId, dialog: result });
      };

      const onLive = (frame: HostFrame): void => {
        switch (frame.type) {
          case "events": {
            let out: ReturnType<typeof dialect.applyEvents>;
            try {
              out = dialect.applyEvents(frame);
            } catch (e) {
              fail(e);
              return;
            }
            if (out.replaced === true) send({ type: "session.window", sessionId, mode: "tail", window: tailWindow() });
            else {
              if (out.append.length > 0) send({ type: "session.append", sessionId, items: out.append });
              if (out.update.length > 0) send({ type: "session.update", sessionId, items: out.update });
              for (const d of out.deltas) send({ type: "session.delta", sessionId, ...d });
            }
            if (out.busy !== undefined) busy = out.busy;
            if (out.agent?.model !== undefined) agentModel = out.agent.model;
            if (out.agent?.thinking !== undefined) agentThinking = out.agent.thinking;
            if (out.queued !== undefined) {
              const ids = new Set(out.queued.map((q) => q.id));
              queued = queued.filter((q) => q.submissionId !== undefined && ids.has(q.submissionId));
            }
            emitFooter(false);
            emitStatus(false);
            return;
          }
          case "snapshot":
            try {
              dialect.applySnapshot(frame);
            } catch (e) {
              fail(e);
              return;
            }
            send({ type: "session.window", sessionId, mode: "tail", window: tailWindow() });
            emitFooter(false);
            return;
          case "host_state": {
            busy = frame.state !== "idle";
            if (frame.agent?.model !== undefined) agentModel = `${frame.agent.model.provider}/${frame.agent.model.model_id}`;
            if (frame.agent?.thinking_level !== undefined) agentThinking = frame.agent.thinking_level;
            emitFooter(false);
            emitStatus(false);
            return;
          }
          case "ui_request":
            onDialogRequest(frame);
            return;
          case "ui_resolved":
            if (dialogs.delete(frame.id)) send({ type: "session.dialogResolved", sessionId, dialogId: frame.id, outcome: frame.outcome });
            return;
          default:
            return; // notice, status, pong, bye (the client's onClose reports bye), attached/result/error are never forwarded
        }
      };

      // The first snapshot is applied as soon as it arrives; every other frame waits until the attach messages are out.
      let snapshotSeen: (() => void) | null = null;
      const firstSnapshot = new Promise<void>((resolve) => {
        snapshotSeen = resolve;
      });
      let snapshotError: unknown = null;
      let applied = false;
      subscriptions.push(
        client.onFrame((frame) => {
          if (disposed) return;
          if (ready) return onLive(frame);
          if (frame.type === "snapshot" && !applied) {
            applied = true;
            try {
              dialect.applySnapshot(frame);
            } catch (e) {
              snapshotError = e;
            }
            snapshotSeen?.();
            return;
          }
          early.push(frame);
        }),
        client.onClose((error) => {
          if (gone !== null) return;
          gone = error instanceof WebError ? error : new WebError("host_disconnected", "durable host connection closed");
          fail(gone);
        }),
      );
      const abortAttach = async (error: unknown): Promise<never> => {
        for (const s of subscriptions) s.dispose();
        client.close();
        throw error;
      };

      try {
        await client.attach();
        const timer = deps.scheduler.after(FIRST_SNAPSHOT_MS, () => {
          snapshotError = new WebError("host_unreachable", "no snapshot after attach", { reason: "no_snapshot" });
          snapshotSeen?.();
        });
        await firstSnapshot;
        timer.dispose();
        if (snapshotError !== null) throw snapshotError;
      } catch (e) {
        return abortAttach(e);
      }

      try {
        models = await client.listModels();
      } catch (e) {
        modelsReason = `model list failed: ${e instanceof Error ? e.message : String(e)}`;
      }
      const cwd = client.welcome.cwd;
      try {
        const name = await deps.git.branch(cwd);
        branch = name === null ? unknown(`${cwd} is not on a git branch`) : known(name);
      } catch (e) {
        branch = unknown(`git branch failed: ${e instanceof Error ? e.message : String(e)}`);
      }

      const initial = footer();
      lastFooter = JSON.stringify(initial);
      send({ type: "session.attached", sessionId, capabilities, title: session.ref.title, meta: { ref: session.ref, footer: initial } });
      send({ type: "session.window", sessionId, mode: "tail", window: tailWindow() });
      send({ type: "session.footer", sessionId, footer: initial });
      emitStatus(true);
      ready = true;
      for (const frame of early.splice(0)) onLive(frame);

      const guard = (): void => {
        if (gone !== null) throw gone;
      };
      const invalid = (message: string): WebError => new WebError("invalid_message", message);

      const handle: SessionHandle = {
        capabilities,
        async loadEarlier(before) {
          guard();
          const m = /^dur:(\d+)$/.exec(before);
          const end = m === null ? NaN : Number(m[1]);
          const items = dialect.transcript.items;
          if (!Number.isSafeInteger(end) || end > items.length) throw invalid(`bad cursor ${before}`);
          const start = Math.max(0, end - windowSize);
          send({
            type: "session.window",
            sessionId,
            mode: "earlier",
            window: { items: items.slice(start, end), hasEarlier: start > 0, cursor: start > 0 ? `dur:${start}` : null },
          });
        },
        async submit(input) {
          guard();
          if (input.text.trim() === "" && input.images.length === 0) {
            const error = new WebError("invalid_submission", "empty submission");
            send({ type: "session.error", sessionId, clientSubmissionId: input.clientSubmissionId, error: toWireError(error) });
            throw error;
          }
          try {
            const result = await client.submit({
              text: input.text,
              whenBusy: input.whenBusy,
              requestId: input.clientSubmissionId,
              ...(input.images.length > 0 ? { images: input.images } : {}),
            });
            const wasBusy = busy;
            send({ type: "session.submitAck", sessionId, clientSubmissionId: input.clientSubmissionId, queued: wasBusy, submissionId: result.submissionId });
            if (wasBusy) {
              queued = [
                ...queued,
                { clientSubmissionId: input.clientSubmissionId, submissionId: result.submissionId, text: input.text.slice(0, 200), whenBusy: input.whenBusy },
              ];
              emitStatus(false);
            }
          } catch (e) {
            send({ type: "session.error", sessionId, clientSubmissionId: input.clientSubmissionId, error: toWireError(e) });
            throw e;
          }
        },
        async abort() {
          guard();
          await client.abort();
        },
        async abortSubmission(clientSubmissionId) {
          guard();
          const entry = queued.find((q) => q.clientSubmissionId === clientSubmissionId);
          if (entry?.submissionId === undefined) throw invalid(`no queued submission ${clientSubmissionId}`);
          await client.abortSubmission(entry.submissionId);
          queued = queued.filter((q) => q !== entry);
          emitStatus(false);
        },
        async compact(instructions) {
          guard();
          await client.compact(instructions);
        },
        async configure(change: ConfigureChange) {
          guard();
          if (change.kind === "tier") {
            await deps.cpd.json(sessionRetierArgs(sessionId, change.tier));
            tier = change.tier;
          } else if (change.kind === "model") {
            await client.configure({ model: change.model });
            agentModel = change.model;
          } else {
            await client.configure({ thinkingLevel: change.level });
            agentThinking = change.level;
          }
          emitFooter(false);
        },
        async listModels() {
          guard();
          const list = await client.listModels();
          models = list;
          const choices: ModelChoice[] = list.map((m) => ({
            model: `${m.provider}/${m.model_id}`,
            name: m.name,
            thinkingLevels: m.thinking_levels,
            contextWindow: m.context_window,
          }));
          send({ type: "session.models", sessionId, models: choices });
          emitFooter(false);
        },
        async respond(dialogId, response: DialogResponse) {
          guard();
          const pending = dialogs.get(dialogId);
          if (pending === undefined) throw invalid(`no open dialog ${dialogId}`);
          if (pending.dialog.method !== response.method) throw invalid(`dialog ${dialogId} is ${pending.dialog.method}, not ${response.method}`);
          if (response.outcome === "cancelled") return client.respond(dialogId, { cancelled: true });
          if (response.method === "ask_user_question") {
            if (response.outcome !== "answered") throw invalid("bad ask_user_question outcome");
            return client.respond(dialogId, { answer: response.result });
          }
          switch (response.outcome) {
            case "approve":
              return client.respond(dialogId, { value: "Approve" });
            case "changes":
              pendingFeedback = response.feedback;
              try {
                await client.respond(dialogId, { value: "Change" });
              } catch (e) {
                pendingFeedback = null;
                throw e;
              }
              return;
            case "reject": {
              const summary = [...dialect.transcript.items].reverse().find((i) => i.kind === "fold" && i.foldKind === "cpd-plan-summary");
              const review = summary?.kind === "fold" ? /^Plan review (.+) summary$/.exec(summary.title)?.[1] : undefined;
              const plan = session.ref.key.split(":")[1];
              if (review === undefined || plan === undefined) throw invalid("no plan review to reject in this session");
              await deps.cpd.json(["plan", "decide", review, "--reject", "--plan", plan]);
              return;
            }
          }
        },
        async dispose() {
          if (disposed) return;
          try {
            if (gone === null) await client.detach();
          } catch {
            // best effort: the host may already be closing the socket
          }
          disposed = true;
          for (const s of subscriptions) s.dispose();
          client.close();
        },
      };
      return handle;
    },
  };
}
