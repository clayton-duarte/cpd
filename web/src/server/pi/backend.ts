import type { LiveSource } from "./live.ts";
import { execFile } from "node:child_process";
import * as nodeFs from "node:fs/promises";
import * as nodePath from "node:path";
import { IMAGE_MAX_COUNT, IMAGE_MAX_TOTAL_BASE64, PI_SEND_TEXT_MAX, WINDOW_SIZE, WebError, toWireError } from "../../protocol/domain.ts";
import type {
  Capabilities,
  ConfigureChange,
  Cursor,
  Dialog,
  DialogResponse,
  FooterStats,
  SessionRef,
  Stat,
  WhenBusy,
  WireImage,
} from "../../protocol/domain.ts";
import type { Disposable, Scheduler } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import type { Cpd } from "../cockpit/cli.ts";
import { toArgv } from "../cpd-argv.ts";
import type { ResolvedSession } from "../hub/registry.ts";
import type { SessionEmit, SessionHandle, SessionBackend } from "../hub/connection.ts";
import type { GitBoundary } from "../../protocol/runtime.ts";
import type { JsonlFacts, PiNormalizer, PushResult } from "./jsonl.ts";
import { watchSession } from "./watch.ts";
import type { WatchBoundary } from "./watch.ts";
import { readEarlier, readTail } from "./window.ts";
import type { TailWindow } from "./window.ts";
import { createNormalizer } from "./jsonl.ts";
import type { FileBoundary } from "./window.ts";

export type { Disposable };

// The chat argv builders of cockpit/actions.ts (CONTRACT 6.3), through the shared builder (docs/contract.md §5.6).
const WEB_UNBLOCKS = "User message from the web chat";
const sessionSendArgs = (id: string, text: string, webuiToken?: string): readonly string[] =>
  toArgv({ kind: "session.send", sessionId: id, text, unblocks: WEB_UNBLOCKS, origin: "webui", ...(webuiToken === undefined ? {} : { webuiToken }) });
const sessionRetierArgs = (id: string, tier: string): readonly string[] =>
  toArgv({ kind: "session.retier", sessionId: id, tier });

const REASON = {
  steer: "`cpd session send` cannot steer a running pi turn; send a follow-up instead",
  abort: "cpd has no command to abort a pi session turn",
  abortSubmission: "pi sessions have no submission queue to cancel from",
  compact: "cpd has no command to compact a pi session",
  model: "cpd cannot change the model of a pi session; pick a Tier instead",
  thinking: "cpd cannot change the thinking level of a pi session",
  dialogs: "a question of a pi session cannot be answered from outside; it is shown read-only, rebuilt from the transcript",
} as const;
const IMAGE_EXT = { "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" } as const;
const no = (reason: string): { supported: false; reason: string } => ({ supported: false, reason });
const unsupported = (reason: string): WebError => new WebError("unsupported_capability", reason);

const known = <T>(value: T): Stat<T> => ({ state: "known", value });
const unknown = (reason: string): Stat<never> => ({ state: "unknown", reason });

interface ShowStats {
  readonly model?: string;
  readonly thinking?: string;
  readonly tier?: string;
  readonly file?: string;
  readonly error?: string;
}

const isBusy = (f: JsonlFacts): boolean =>
  f.lastRole === "user" ||
  f.lastRole === "toolResult" ||
  (f.lastRole === "assistant" && (f.lastStopReason === "toolUse" || f.lastStopReason === "pending"));

export function createPiBackend(deps: {
  cpd: Cpd;
  fs: FileBoundary;
  watch: WatchBoundary;
  git: GitBoundary;
  cpdHome: string;
  /** This boot's WebUI token (contract §6.16): sent to `cpd session send --origin webui`. */
  webuiToken?: string;
  attachmentsDir: string;
  now(): number;
  scheduler: Scheduler;
  windowSize?: number;
  /** One source per attach: pass a factory when the backend serves several sessions. */
  live?: LiveSource | (() => LiveSource);
}): SessionBackend {
  const windowSize = deps.windowSize ?? WINDOW_SIZE;

  async function attach(session: ResolvedSession, emit: SessionEmit): Promise<SessionHandle> {
    const { ref } = session;
    let currentFile: string | undefined = session.file;
    const sessionId = ref.id;
    const emptyTail = (): TailWindow => ({
      window: { items: [], hasEarlier: false, cursor: null },
      startOffset: 0,
      endOffset: 0,
      identity: "",
      normalizer: createNormalizer(),
      openDialogs: [],
    });
    let tail = currentFile === undefined ? emptyTail() : await readTail(currentFile, deps.fs, windowSize);

    const readShow = async (): Promise<ShowStats> => {
      try {
        const reply = await deps.cpd.json(toArgv({ kind: "session.show", sessionId }));
        const row = reply.session;
        const o = typeof row === "object" && row !== null ? (row as Record<string, unknown>) : {};
        const s = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);
        return { model: s(o.model), thinking: s(o.thinking), tier: s(o.tier), file: s(o.file) };
      } catch (e) {
        return { error: e instanceof Error ? e.message : String(e) };
      }
    };
    let show = await readShow();

    let branch: Stat<string>;
    if (ref.cwd === undefined) branch = unknown("the session row has no cwd");
    else {
      try {
        const b = await deps.git.branch(ref.cwd);
        branch = b === null ? unknown("not a git repository") : known(b);
      } catch (e) {
        branch = unknown(e instanceof Error ? e.message : String(e));
      }
    }

    const pick = (fact: string | undefined, shown: string | undefined, what: string): Stat<string> => {
      const v = fact ?? shown;
      if (v !== undefined) return known(v);
      return unknown(show.error === undefined ? `cpd session show has no ${what}` : `cpd session show failed: ${show.error}`);
    };
    const footer = (): FooterStats => {
      const facts = tail.normalizer.facts;
      const tier = show.tier ?? ref.tier;
      return {
        model: pick(facts.model, show.model, "model"),
        tier: tier === undefined ? unknown("no Tier known for this session") : known(tier),
        thinking: pick(facts.thinking, show.thinking, "thinking level"),
        contextPercent: unknown("the model's context window is not available for pi sessions"),
        cost: unknown("pi session cost needs a full-file scan, which Phase 1 never does"),
        branch,
      };
    };

    const capabilities: Capabilities = {
      images: { supported: true, maxCount: IMAGE_MAX_COUNT, maxTotalBase64: IMAGE_MAX_TOTAL_BASE64 },
      steer: no(REASON.steer),
      followUp: { supported: true },
      abort: no(REASON.abort),
      abortSubmission: no(REASON.abortSubmission),
      compact: no(REASON.compact),
      configure: {
        model: no(REASON.model),
        thinking: no(REASON.thinking),
        tier: { supported: true, options: session.tierOptions },
      },
      dialogs: { mode: "interactive" },
    };

    let disposed = false;
    const live = typeof deps.live === "function" ? deps.live() : deps.live;
    // `message.timestamp` of the committed assistant lines, and the `live:` items sent and not yet removed.
    const committed = new Set<number>();
    const liveIds = new Set<string>();
    let turnsSeen = 0;
    const syncTurns = (): number[] => {
      const fresh = tail.normalizer.turns.slice(turnsSeen);
      turnsSeen = tail.normalizer.turns.length;
      for (const ts of fresh) committed.add(ts);
      return fresh;
    };
    syncTurns();
    let busy = isBusy(tail.normalizer.facts);
    let watcher: Disposable | undefined;
    const out = (m: SessionServerMessage): void => {
      if (!disposed) emit(m);
    };
    const status = (): void => out({ type: "session.status", sessionId, busy, queued: [] });
    const openReviews = new Set<string>();
    const openQuestions = new Set<string>();
    const dialog = (d: Dialog): void => {
      if (d.method === "cpd_plan_review") openReviews.add(d.id);
      if (d.method === "ask_user_question") openQuestions.add(d.id);
      out({ type: "session.dialog", sessionId, dialog: d });
    };

    const startWatch = (normalizer: PiNormalizer, offset: number, identity: string): void => {
      if (currentFile === undefined) return;
      watcher = watchSession({
        path: currentFile,
        fs: deps.fs,
        watch: deps.watch,
        normalizer,
        offset,
        identity,
        onEvent: (e) => {
          if (e.kind === "reset") {
            watcher?.dispose();
            void reload().catch((err: unknown) => out({ type: "session.error", sessionId, error: toWireError(err) }));
          } else applyAppend(e.result);
        },
        onError: (error) => out({ type: "session.error", sessionId, error: toWireError(error) }),
      });
    };

    const applyAppend = (r: PushResult): void => {
      for (const ts of syncTurns()) if (liveIds.delete(`live:${ts}`)) out({ type: "session.remove", sessionId, itemId: `live:${ts}` });
      if (r.added.length > 0) out({ type: "session.append", sessionId, items: r.added });
      if (r.updated.length > 0) out({ type: "session.update", sessionId, items: r.updated });
      for (const d of r.dialogsOpened) dialog(d);
      for (const id of r.dialogsResolved) {
        openReviews.delete(id);
        openQuestions.delete(id);
      }
      for (const id of r.dialogsResolved) out({ type: "session.dialogResolved", sessionId, dialogId: id, outcome: "answered" });
      if (r.factsChanged) out({ type: "session.footer", sessionId, footer: footer() });
      const now = isBusy(tail.normalizer.facts);
      if (now !== busy) {
        busy = now;
        status();
      }
    };

    const emitTail = (): void => {
      out({ type: "session.window", sessionId, mode: "tail", window: tail.window });
      out({ type: "session.footer", sessionId, footer: footer() });
      status();
      for (const d of tail.openDialogs) dialog(d);
    };

    async function reload(): Promise<void> {
      const next = await readTail(currentFile as string, deps.fs, windowSize);
      if (disposed) return;
      tail = next;
      committed.clear();
      liveIds.clear();
      turnsSeen = 0;
      syncTurns();
      busy = isBusy(tail.normalizer.facts);
      emitTail();
      startWatch(tail.normalizer, tail.endOffset, tail.identity);
    }

    emit({ type: "session.attached", sessionId, capabilities, title: ref.title, meta: { ref, footer: footer() } });
    emitTail();
    startWatch(tail.normalizer, tail.endOffset, tail.identity);
    live?.attach(
      sessionId,
      (m) => {
        if (m.type === "session.append") for (const i of m.items) if (i.id.startsWith("live:")) liveIds.add(i.id);
        if (m.type === "session.remove") liveIds.delete(m.itemId);
        out(m);
      },
      (ts) => committed.has(ts),
    );

    const POLL_MS = 250;
    const POLL_MAX = 20;
    let poll: Disposable | undefined;
    const pollError = (error: unknown): void => out({ type: "session.error", sessionId, error: toWireError(error) });
    const startPoll = (): void => {
      let ticks = 0;
      const arm = (): void => {
        poll = deps.scheduler.after(POLL_MS, () => {
          void tick().catch(pollError);
        });
      };
      const tick = async (): Promise<void> => {
        ticks += 1;
        const shown = await readShow();
        if (disposed) return;
        if (shown.file !== undefined) {
          currentFile = shown.file;
          poll = undefined;
          await reload();
          return;
        }
        if (ticks >= POLL_MAX) {
          poll = undefined;
          pollError(new WebError("session_file_missing", `session ${sessionId} has no session file`, { sessionId }));
          return;
        }
        arm();
      };
      arm();
    };

    return {
      capabilities,
      async loadEarlier(before) {
        if (currentFile === undefined) {
          out({ type: "session.window", sessionId, mode: "earlier", window: { items: [], hasEarlier: false, cursor: null } });
          return;
        }
        const page = await readEarlier(currentFile, deps.fs, before, windowSize);
        out({ type: "session.window", sessionId, mode: "earlier", window: page.window });
      },
      async submit(input) {
        if (input.whenBusy === "steer") throw unsupported(REASON.steer);
        if (input.text.trim() === "" && input.images.length === 0) throw new WebError("invalid_submission", "the message is empty");
        const stamp = deps.now();
        const paths = input.images.map((im, i) => `${deps.attachmentsDir}/${sessionId}-${stamp}-${i}.${IMAGE_EXT[im.mimeType]}`);
        const attached = paths.map((p) => `[Attached image: ${p}]`).join("\n");
        const text = paths.length === 0 ? input.text : input.text.trim() === "" ? attached : `${input.text}\n\n${attached}`;
        if (text.length > PI_SEND_TEXT_MAX) {
          throw new WebError("invalid_submission", `the message is longer than ${PI_SEND_TEXT_MAX} characters`);
        }
        for (const [i, im] of input.images.entries()) {
          await deps.fs.write(paths[i] as string, Uint8Array.from(Buffer.from(im.dataBase64, "base64")));
        }
        await deps.cpd.json(sessionSendArgs(sessionId, text, deps.webuiToken));
        out({ type: "session.submitAck", sessionId, clientSubmissionId: input.clientSubmissionId, queued: false });
        if (currentFile === undefined && poll === undefined) startPoll();
      },
      abort: () => Promise.reject(unsupported(REASON.abort)),
      abortSubmission: () => Promise.reject(unsupported(REASON.abortSubmission)),
      compact: () => Promise.reject(unsupported(REASON.compact)),
      async configure(change) {
        if (change.kind === "model") throw unsupported(REASON.model);
        if (change.kind === "thinking") throw unsupported(REASON.thinking);
        if (!session.tierOptions.includes(change.tier)) {
          throw new WebError("invalid_message", `unknown Tier ${change.tier}`, { field: "tier" });
        }
        await deps.cpd.json(sessionRetierArgs(sessionId, change.tier));
        show = await readShow();
        out({ type: "session.footer", sessionId, footer: footer() });
      },
      listModels: () => Promise.reject(unsupported(REASON.model)),
      async respond(dialogId, response) {
        if (response.method === "ask_user_question") {
          openQuestions.delete(dialogId);
          out({
            type: "session.dialogResolved",
            sessionId,
            dialogId,
            outcome: response.outcome === "answered" ? "answered" : "cancelled",
          });
          const askPath = `${deps.cpdHome}/run/ask/${sessionId}.json`;
          const payload = {
            dialogId,
            result:
              response.outcome === "answered"
                ? response.result
                : { answers: [], cancelled: true },
          };
          try {
            await nodeFs.mkdir(nodePath.dirname(askPath), { recursive: true });
          } catch {
            // ignore
          }
          await deps.fs.write(
            askPath,
            new TextEncoder().encode(JSON.stringify(payload)),
          );
          void (async () => {
            await new Promise((r) => setTimeout(r, 350));
            try {
              await nodeFs.stat(askPath);
              await nodeFs.rm(askPath, { force: true });
              const shown = await deps.cpd.json(toArgv({ kind: "session.show", sessionId }));
              const pane =
                typeof (shown["session"] as Record<string, unknown> | undefined)?.["tmux_pane"] === "string"
                  ? ((shown["session"] as Record<string, unknown>)["tmux_pane"] as string)
                  : null;
              if (pane !== null) {
                await new Promise<void>((resolve) => {
                  execFile("tmux", ["send-keys", "-t", pane, "Escape"], () => resolve());
                });
              }
              if (response.outcome === "answered") {
                const summary = response.result.answers
                  .map((a) => {
                    const val =
                      a.kind === "multi"
                        ? [...(a.selected ?? []), ...(a.answer ? [a.answer] : [])].join(", ")
                        : a.answer;
                    return `${a.question} => ${val}`;
                  })
                  .join("\n");
                if (summary.trim() !== "") {
                  await deps.cpd.json(sessionSendArgs(sessionId, summary, deps.webuiToken));
                }
              }
            } catch {
              // askPath was already consumed by pi/ask-user-question
            }
          })();
          return;
        }
        if (response.outcome === "cancelled") return;
        if (!openReviews.has(dialogId)) throw new WebError("invalid_message", `no pending plan review ${dialogId}`, { field: "dialogId" });
        const n = dialogId.slice("review:".length);
        const plan = ref.key.split(":")[1] as string;
        const decide = (decision: "approve" | "reject", feedbackFile?: string): string[] =>
          toArgv({ kind: "plan.decide", review: n, decision, planId: plan, feedbackFile });
        if (response.outcome === "approve") await deps.cpd.json(decide("approve"));
        else if (response.outcome === "reject") await deps.cpd.json(decide("reject"));
        else {
          const path = `${deps.cpdHome}/run/${plan}/feedback-${n}.md`;
          await deps.fs.write(path, new TextEncoder().encode(response.feedback));
          await deps.cpd.json(decide("reject", path));
        }
      },
      dispose() {
        disposed = true;
        live?.detach();
        poll?.dispose();
        watcher?.dispose();
        return Promise.resolve();
      },
    };
  }

  return { attach };
}
