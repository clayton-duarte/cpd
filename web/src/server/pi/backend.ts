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
import type { Disposable } from "../../protocol/runtime.ts";
import type { SessionServerMessage } from "../../protocol/wire.ts";
import type { Cpd } from "../cockpit/cli.ts";
import type { ResolvedSession } from "../hub/registry.ts";
import type { SessionEmit, SessionHandle, SessionBackend } from "../hub/connection.ts";
import type { GitBoundary } from "../../protocol/runtime.ts";
import type { JsonlFacts, PiNormalizer, PushResult } from "./jsonl.ts";
import { watchSession } from "./watch.ts";
import type { WatchBoundary } from "./watch.ts";
import { readEarlier, readTail } from "./window.ts";
import type { FileBoundary } from "./window.ts";

export type { Disposable };

// Local copies of the argv builders of cockpit/actions.ts (sessionSendArgs, sessionRetierArgs, CONTRACT 6.3).
const WEB_UNBLOCKS = "User message from the web chat";
const sessionSendArgs = (id: string, text: string): readonly string[] => ["session", "send", id, "--unblocks", WEB_UNBLOCKS, "--", text];
const sessionRetierArgs = (id: string, tier: string): readonly string[] => ["session", "retier", id, "--tier", tier];

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
  attachmentsDir: string;
  now(): number;
  windowSize?: number;
}): SessionBackend {
  const windowSize = deps.windowSize ?? WINDOW_SIZE;

  async function attach(session: ResolvedSession, emit: SessionEmit): Promise<SessionHandle> {
    const { ref } = session;
    const file = session.file;
    if (file === undefined) throw new WebError("session_file_missing", `session ${ref.id} has no session file`, { sessionId: ref.id });
    const sessionId = ref.id;
    let tail = await readTail(file, deps.fs, windowSize);

    const readShow = async (): Promise<ShowStats> => {
      try {
        const reply = await deps.cpd.json(["session", "show", sessionId]);
        const row = reply.session;
        const o = typeof row === "object" && row !== null ? (row as Record<string, unknown>) : {};
        const s = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);
        return { model: s(o.model), thinking: s(o.thinking), tier: s(o.tier) };
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
    let busy = isBusy(tail.normalizer.facts);
    let watcher: Disposable | undefined;
    const out = (m: SessionServerMessage): void => {
      if (!disposed) emit(m);
    };
    const status = (): void => out({ type: "session.status", sessionId, busy, queued: [] });
    const openReviews = new Set<string>();
    const dialog = (d: Dialog): void => {
      if (d.method === "cpd_plan_review") openReviews.add(d.id);
      out({ type: "session.dialog", sessionId, dialog: d });
    };

    const startWatch = (normalizer: PiNormalizer, offset: number, identity: string): void => {
      watcher = watchSession({
        path: file,
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
      if (r.added.length > 0) out({ type: "session.append", sessionId, items: r.added });
      if (r.updated.length > 0) out({ type: "session.update", sessionId, items: r.updated });
      for (const d of r.dialogsOpened) dialog(d);
      for (const id of r.dialogsResolved) openReviews.delete(id);
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
      const next = await readTail(file as string, deps.fs, windowSize);
      if (disposed) return;
      tail = next;
      busy = isBusy(tail.normalizer.facts);
      emitTail();
      startWatch(tail.normalizer, tail.endOffset, tail.identity);
    }

    emit({ type: "session.attached", sessionId, capabilities, title: ref.title, meta: { ref, footer: footer() } });
    emitTail();
    startWatch(tail.normalizer, tail.endOffset, tail.identity);

    return {
      capabilities,
      async loadEarlier(before) {
        const page = await readEarlier(file, deps.fs, before, windowSize);
        out({ type: "session.window", sessionId, mode: "earlier", window: page.window });
      },
      async submit(input) {
        if (input.whenBusy === "steer") throw unsupported(REASON.steer);
        if (input.text.trim() === "") throw new WebError("invalid_submission", "the message is empty");
        const stamp = deps.now();
        const paths = input.images.map((im, i) => `${deps.attachmentsDir}/${sessionId}-${stamp}-${i}.${IMAGE_EXT[im.mimeType]}`);
        const text = paths.length === 0 ? input.text : `${input.text}\n\n${paths.map((p) => `[Attached image: ${p}]`).join("\n")}`;
        if (text.length > PI_SEND_TEXT_MAX) {
          throw new WebError("invalid_submission", `the message is longer than ${PI_SEND_TEXT_MAX} characters`);
        }
        for (const [i, im] of input.images.entries()) {
          await deps.fs.write(paths[i] as string, Uint8Array.from(Buffer.from(im.dataBase64, "base64")));
        }
        await deps.cpd.json(sessionSendArgs(sessionId, text));
        out({ type: "session.submitAck", sessionId, clientSubmissionId: input.clientSubmissionId, queued: false });
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
        if (response.method === "ask_user_question") throw unsupported(REASON.dialogs);
        if (response.outcome === "cancelled") return;
        if (!openReviews.has(dialogId)) throw new WebError("invalid_message", `no pending plan review ${dialogId}`, { field: "dialogId" });
        const n = dialogId.slice("review:".length);
        const plan = ref.key.split(":")[1] as string;
        if (response.outcome === "approve") await deps.cpd.json(["plan", "decide", n, "--approve", "--plan", plan]);
        else if (response.outcome === "reject") await deps.cpd.json(["plan", "decide", n, "--reject", "--plan", plan]);
        else {
          const path = `${deps.cpdHome}/run/${plan}/feedback-${n}.md`;
          await deps.fs.write(path, new TextEncoder().encode(response.feedback));
          await deps.cpd.json(["plan", "decide", n, "--reject", "--feedback-file", path, "--plan", plan]);
        }
      },
      dispose() {
        disposed = true;
        watcher?.dispose();
        return Promise.resolve();
      },
    };
  }

  return { attach };
}
