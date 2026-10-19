import { Text } from "@earendil-works/pi-tui";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { callCpd, CpdError, cpdProtocol, installedIdentity, isObject, toCpdError } from "./cpd.ts";
import { cursorFrom } from "./cursor.ts";
import { errorMessage, eventMessage, FOLLOW_UP, LEAD_MESSAGES, sessionMessage } from "./delivery.ts";
import type { CpdEvent } from "./event.ts";
import { parseLine, PROTOCOL } from "./event.ts";
import { DEFAULT_TIMING, Follower, PERMANENT_CODES, type Schedule, type Spawn, type Timing } from "./follower.ts";
import type { CommandCtx, Ctx, Exec, Message, MessageRenderer, PiApi } from "./pi.ts";
import { lastAssistantText, needsReminder, planKeys, REMINDER_TEXT, REMINDER_TYPE } from "./reminder.ts";
import { parseOpened, parseResult, parseStatus, request } from "./plannotator.ts";
import { DECISION_TYPE, decisionText, ENTRY_TYPE, lostText, highestReview, REVIEW_LABEL, reviewNumber, reviewsFromEntries, type ReviewEntry } from "./plan-review.ts";
import { adapterDir, appendLog, DEFAULT_POLL_MS, LONG_REFUSAL_NOTICE, pollAdapter, type Poll, refusal, RELOAD_COMMAND, reloadLogPath, RELOAD_TRIGGER, validatePackage, type Validation } from "./reload.ts";
import { isTyped, startsReply } from "./reply.ts";
import { Facts, Reporter, stateArgs, type Wanted } from "./state.ts";
import { CONTEXT_TYPE, contextCursorFrom, contextUpdate, countsOf, parsePending, parsePlanEvents, parseRecordEvents, parseRecords, PLAN_KINDS, summaryLine } from "./updates.ts";

export type Options = {
  /** The environment read at `session_start` (`CPD_SESSION`, `CPD_PLAN`, `CPD_HOME`). Default: `process.env`. */
  env?: Record<string, string | undefined>;
  timing?: Partial<Timing>;
  spawn?: Spawn;
  /** How the follower's restart is scheduled. Default: `setTimeout`. */
  schedule?: Schedule;
  /** The directory whose replacement means an install, how often it is polled, and how the new package is validated (§15.6). Default: the adapter's own directory, every {@link DEFAULT_POLL_MS}, jiti from pi. */
  reload?: { dir?: string; intervalMs?: number; longRefusalMs?: number } & Validation;
};

/** commands.md §15.7 "Display": collapsed, one dim line of counts; expanded, the full text. The model gets `content` either way. A message without counts (an older adapter's) is shown in full. */
export const renderContext: MessageRenderer = (message, { expanded, outputPad }, theme) => {
  const counts = countsOf(message.details);
  const text = expanded || counts === undefined ? String(message.content) : theme.fg("dim", summaryLine(counts));
  return new Text(text, outputPad, 0, (t) => theme.bg("customMessageBg", t));
};

export const QUESTION_LABEL = "cpd: Worker question";

type SessionInfo = { role: string; context: string; plan: string | null; planLead: boolean; registeredEvent: number; name: string | null };

/** One activation of the adapter: from `session_start` to `session_shutdown`. */
type Run = {
  id: string;
  info: SessionInfo;
  /** Highest Event id handled; the next `--since`. */
  cursor: number;
  facts: Facts;
  reporter: Reporter;
  follower: Follower;
  reviews: Map<string, ReviewEntry>;
  opening: Set<string>;
  deciding: Set<string>;
  /** Error codes already delivered since the last valid Event (commands.md §15.2, "The follower ends"). */
  followerReported: Set<string>;
  failureLineThisRun: boolean;
  /** Events are handled one after the other, in id order, though a handler may wait for `cpd`. */
  tail: Promise<void>;
  /** True while a failed read of Context updates is reported and no read has succeeded since (§15.7 "Failure"). */
  contextFailed: boolean;
  /** The name last known to be the Session's, so `session_info_changed` can tell a User's rename from ours. */
  knownName: string | null;
  /** False until the start-up's `session state idle` is done: facts are kept, nothing is reported before. */
  reporting: boolean;
  abort: AbortController;
  /** Watches the adapter directory for an install (§15 "Reload after an install"). */
  reload: Poll | undefined;
  /** §15.8: the last run ended on a message that spoke of further work and named no registered key; the next `before_agent_start` adds the reminder once. */
  reminderDue: boolean;
  /** One line of `run/adapter/<session>.log` (§15.6 "Log"); a failed write is notified once per run. */
  log: (line: string) => void;
  unsubscribe: (() => void)[];
};

const asString = (v: unknown): string | null => (typeof v === "string" ? v : null);

function parseSession(show: Record<string, unknown>): SessionInfo {
  const s = show.session;
  if (!isObject(s) || typeof s.role !== "string" || typeof s.context !== "string" || typeof s.plan_lead !== "boolean" || !Number.isSafeInteger(s.registered_event) || (s.plan !== null && typeof s.plan !== "string") || (s.name !== null && typeof s.name !== "string")) {
    throw new CpdError("bad_output", "cpd session show: the session object lacks role, context, plan, plan_lead, registered_event or name");
  }
  if (s.plan_lead && typeof s.plan !== "string") throw new CpdError("bad_output", "cpd session show: a Plan Lead without a plan");
  return { role: s.role, context: s.context, plan: asString(s.plan), planLead: s.plan_lead, registeredEvent: s.registered_event as number, name: asString(s.name) };
}

export function adapter(pi: PiApi, options: Options = {}): void {
  const timing: Timing = { ...DEFAULT_TIMING, ...options.timing };
  const env = options.env ?? process.env;
  let run: Run | undefined;
  let toolRegistered = false;
  // Set while the start-up `session show` is being retried (§15.2 step 1); `stop` aborts it.
  let showRetry: AbortController | undefined;
  // The context of `session_start`: `ctx.ui` is only handed to handlers.
  const ctxOf: { current: Ctx | undefined } = { current: undefined };

  // Aborted at `session_shutdown` (and by the next start): a call in flight then ends instead of outliving the run.
  let abort = new AbortController();
  const exec: Exec = (command, args, options) => pi.exec(command, args, options);
  const call = (args: string[]) => callCpd(exec, args, { timeoutMs: timing.callTimeoutMs, signal: abort.signal });

  // The installed cpd binaries already reported as speaking another protocol (commands.md §15.2, step 0), and the
  // error codes of a failed `--version` already reported; both once per adapter, so a follower restart repeats nothing.
  const mismatched = new Set<string>();
  const versionFailures = new Set<string>();

  /** commands.md §15.2, step 0. Never throws; the adapter keeps following whatever it finds. */
  async function checkProtocol(): Promise<void> {
    try {
      const cpd = await cpdProtocol(exec, { timeoutMs: timing.callTimeoutMs, signal: abort.signal });
      versionFailures.clear();
      if (cpd === PROTOCOL) return;
      const identity = installedIdentity() ?? "cpd";
      if (mismatched.has(identity)) return;
      mismatched.add(identity);
      const speaks = cpd === null ? "no protocol (`cpd --version` has no `protocol`)" : `protocol ${cpd}`;
      const m = errorMessage("protocol mismatch", "protocol_mismatch", `cpd speaks ${speaks}, this adapter ${PROTOCOL}; reload pi (/reload)`, { cpd, adapter: PROTOCOL });
      pi.sendMessage(m.message, m.options);
    } catch (e) {
      const err = toCpdError(e);
      if (versionFailures.has(err.code)) return;
      versionFailures.add(err.code);
      deliver("protocol check", err);
    }
  }

  const deliver = (what: string, e: unknown, extra: Record<string, unknown> = {}) => {
    const err = toCpdError(e);
    const m = errorMessage(what, err.code, err.message, { ...err.details, ...extra });
    pi.sendMessage(m.message, m.options);
  };

  // ---- agent state (§15.4) ----

  const refresh = (r: Run) => {
    if (r.reporting && run === r) r.reporter.want(r.facts.wanted());
  };

  const newReporter = (id: string, facts: Facts) =>
    new Reporter(
      // No signal: the `stopped` of `session_shutdown` is sent after the abort, and `stop()` bounds its wait.
      async (w: Wanted) => {
        if (w.state === "idle" && w.reply) facts.reply = false; // sent once, with this call
        await callCpd(exec, stateArgs(w, id), { timeoutMs: timing.callTimeoutMs });
      },
      (e) => deliver("session state", e),
    );

  pi.registerMessageRenderer(CONTEXT_TYPE, renderContext);

  // ---- reload after an install ----

  const guardOf = (r: Run, ctx: Ctx) => ({ idle: ctx.isIdle(), pending: ctx.hasPendingMessages(), state: r.facts.wanted().state, editorText: ctx.ui.getEditorText() });

  pi.registerCommand(RELOAD_COMMAND, {
    description: "cpd: reload pi after an install (runs by itself; does nothing unless pi is idle, unblocked and the editor is empty)",
    async handler(_args: string, ctx: CommandCtx) {
      const r = run;
      if (!r) return;
      const why = refusal(guardOf(r, ctx));
      if (why !== undefined) {
        r.log(`command: refused: ${why}`);
        return r.reload?.retry();
      }
      r.log("command: reloading");
      ctx.ui.notify("cpd: adapter updated, reloading");
      try {
        await ctx.reload(); // `ctx` is stale from here on
      } finally {
        r.log("command: ctx.reload() returned"); // a reload that happened is followed by the new instance's "start" line
        r.reload?.retry(); // still running: the reload did not happen, the next tick may try again
      }
    },
  });

  // ---- start-up (§15.2) ----

  async function start(ctx: Ctx, reason: string): Promise<void> {
    if (ctx.mode !== "tui") return;
    const id = env.CPD_SESSION;
    if (id === undefined || id === "") return;
    await stop(); // a start without a shutdown (never seen) must not leave a second follower behind
    abort = new AbortController();
    ctxOf.current = ctx;
    if (ctx.sessionManager.getSessionId() !== id) {
      const m = errorMessage("activation", "session_mismatch", `CPD_SESSION is ${id} but pi's session is ${ctx.sessionManager.getSessionId()}; the adapter stays inactive.`);
      pi.sendMessage(m.message, m.options);
      return;
    }

    // 1. who am I
    let info: SessionInfo;
    try {
      info = await showSession(id);
    } catch (e) {
      const err = toCpdError(e);
      deliver("session show", err);
      if (PERMANENT_CODES.has(err.code)) return;
      const retry = (showRetry = new AbortController());
      void retryShow(id, retry, err.code)
        .then((shown) => (shown ? begin(ctx, id, shown, reason) : undefined))
        .catch((e) => deliver("start-up", e));
      return;
    }
    await begin(ctx, id, info, reason);
  }

  const showSession = async (id: string) => parseSession(await call(["session", "show", id]));

  /** §15.2 step 1 again, with the follower's back-off, until it works, is refused for good, or the run is stopped (then `undefined`). A code is reported once. */
  async function retryShow(id: string, retry: AbortController, reported: string): Promise<SessionInfo | undefined> {
    const seen = new Set([reported]);
    let delay = timing.restartInitialMs;
    for (;;) {
      await sleep(delay, undefined, { signal: retry.signal }).catch(() => undefined);
      if (retry.signal.aborted) return undefined;
      delay = Math.min(delay * 2, timing.restartMaxMs);
      try {
        const info = await showSession(id);
        if (retry.signal.aborted) return undefined;
        showRetry = undefined;
        return info;
      } catch (e) {
        if (retry.signal.aborted) return undefined;
        const err = toCpdError(e);
        if (!seen.has(err.code)) {
          seen.add(err.code);
          deliver("session show", err);
        }
        if (PERMANENT_CODES.has(err.code)) {
          showRetry = undefined;
          return undefined;
        }
      }
    }
  }

  /** §15.2 steps 2 to 6, once the Session is known. */
  async function begin(ctx: Ctx, id: string, info: SessionInfo, reason: string): Promise<void> {
    const facts = new Facts();
    let logFailed = false;
    const r: Run = {
      id,
      info,
      cursor: 0,
      facts,
      reporter: newReporter(id, facts),
      follower: undefined as unknown as Follower,
      reviews: new Map(),
      opening: new Set(),
      deciding: new Set(),
      followerReported: new Set(),
      failureLineThisRun: false,
      tail: Promise.resolve(),
      contextFailed: false,
      knownName: info.name,
      reporting: false,
      abort,
      reload: undefined,
      reminderDue: false,
      log: (line) => {
        try {
          appendLog(reloadLogPath(env.CPD_HOME || join(homedir(), ".cpd"), id), line);
        } catch (e) {
          if (!logFailed) {
            logFailed = true;
            ctxOf.current?.ui.notify(`cpd: cannot write the reload log: ${toCpdError(e).message}`, "error"); // not a message: it must not start a turn
          }
        }
      },
      unsubscribe: [],
    };
    r.follower = new Follower(
      id,
      {
        before: checkProtocol,
        cursor: () => r.cursor,
        onRunStart: () => {
          r.failureLineThisRun = false;
        },
        onLine: (line) => handleLine(r, line),
        onFailure: (e) => {
          if (e.code === "follower_exit" && r.failureLineThisRun) return; // the failure line already said why
          followerError(r, e);
        },
      },
      timing,
      options.spawn,
      options.schedule,
    );
    run = r;
    r.log(`start: ${reason}`);

    // 2. stopped -> idle, then working when pi is busy.
    r.reporter.want({ state: "idle" });
    await r.reporter.settled();
    if (run !== r) return;
    r.reporting = true;
    facts.active = !ctx.isIdle();

    // 3. name
    if (info.name !== null && pi.getSessionName() !== info.name) pi.setSessionName(info.name);

    // 4. cursor
    const entries = ctx.sessionManager.getEntries();
    r.cursor = cursorFrom(info.registeredEvent, entries, id);

    if (info.role === "manager") registerUpdates();

    // 5. blocking reasons (Plan Lead only; a Manager has none and opens no plannotator)
    if (info.role !== "manager" && info.planLead && info.plan !== null) {
      const plan = info.plan;
      const folded = reviewsFromEntries(entries, plan);
      if (folded.invalid > 0) deliver("session entries", new CpdError("entry_invalid", `${folded.invalid} ${ENTRY_TYPE} entries of this session do not parse and were skipped`));
      r.reviews = folded.reviews;
      let latest: string | undefined = highestReview(r.reviews.keys()); // stands in when `plan show` fails (§15.5 "Latest only")
      try {
        const show = await call(["plan", "show", "--plan", plan]);
        latest = isObject(show.review) && typeof show.review.id === "string" ? show.review.id : undefined;
        if (Array.isArray(show.escalations)) {
          for (const q of show.escalations) if (isObject(q) && typeof q.id === "string") facts.open(`escalation:${q.id}`, QUESTION_LABEL);
        }
      } catch (e) {
        deliver("plan show", e);
      }
      for (const entry of r.reviews.values()) {
        if (entry.status === "pending" && entry.review === latest) facts.open(`plan-review:${entry.review}`, REVIEW_LABEL);
      }
      registerTool();
    }
    if (run !== r) return;
    refresh(r);

    // 6. follow
    r.follower.start();
    r.reload = pollAdapter({
      dir: options.reload?.dir ?? adapterDir(),
      intervalMs: options.reload?.intervalMs ?? DEFAULT_POLL_MS,
      validate: (dir) => validatePackage(dir, options.reload),
      refusal: () => (run === r && ctxOf.current !== undefined ? refusal(guardOf(r, ctxOf.current)) : "not active"),
      trigger: () => pi.sendUserMessage(RELOAD_TRIGGER, { expandPromptTemplates: true }),
      onRefused: (reason) => ctxOf.current?.ui.notify(`cpd: adapter update not loaded: ${reason}`, "error"),
      onError: (e) => deliver("reload after install", e),
      log: r.log,
      onLongRefusal: () => ctxOf.current?.ui.notify(LONG_REFUSAL_NOTICE),
      longRefusalMs: options.reload?.longRefusalMs,
    });

    // Recovery of the reviews this session opened (§15.5), in the background.
    r.unsubscribe.push(
      pi.events.on("plannotator:review-result", (data) => {
        const result = parseResult(data);
        if (!result || run !== r) return;
        const entry = [...r.reviews.values()].find((e) => e.status === "pending" && e.plannotator_id === result.reviewId);
        if (entry) void decide(r, entry, result.approved, result.feedback).catch((e) => deliver("plan decision", e));
      }),
    );
    for (const entry of r.reviews.values()) if (entry.status === "pending") void recover(r, entry.review).catch((e) => deliver("plan review recovery", e));
  }

  async function stop(): Promise<void> {
    showRetry?.abort();
    showRetry = undefined;
    abort.abort(); // also a start still waiting for `session show`
    const r = run;
    if (!r) return;
    run = undefined;
    ctxOf.current = undefined;
    r.log("stop");
    r.reload?.stop();
    r.follower.stop();
    r.unsubscribe.forEach((u) => u());
    if (r.reporting) {
      await Promise.race([r.reporter.close({ state: "stopped" }), sleep(timing.shutdownWaitMs, undefined, { ref: false })]);
    }
  }

  // ---- the follower's lines (§15.2, §15.3) ----

  function followerError(r: Run, e: CpdError): void {
    if (r.followerReported.has(e.code)) return;
    r.followerReported.add(e.code);
    deliver("events follow", e);
  }

  /** Returns true when the line was a valid Event. The cursor moves past every line that has an id, delivered or not. */
  function handleLine(r: Run, line: string): boolean {
    const parsed = parseLine(line);
    if (parsed.type === "failure") {
      r.failureLineThisRun = true;
      followerError(r, parsed.error);
      if (PERMANENT_CODES.has(parsed.error.code)) r.follower.stop(); // a refusal: restarting would fail the same way (§15.2)
      return false;
    }
    if (parsed.type === "unknown-kind") {
      r.cursor = Math.max(r.cursor, parsed.id); // skipped silently: a kind this adapter does not know is one it cannot need
      return false;
    }
    if (parsed.type === "invalid") {
      deliver("events follow", new CpdError("invalid_event", `skipped a line that is not a valid Event: ${parsed.reason}`));
      if (parsed.id !== undefined) r.cursor = Math.max(r.cursor, parsed.id);
      return false;
    }
    r.followerReported.clear();
    const event = parsed.event;
    r.tail = r.tail.then(async () => {
      if (run !== r) return;
      try {
        await handleEvent(r, event);
      } catch (e) {
        deliver(`event ${event.id} (${event.kind})`, e);
      }
    });
    r.cursor = Math.max(r.cursor, event.id);
    return true;
  }

  async function handleEvent(r: Run, ev: CpdEvent): Promise<void> {
    const own = ev.session === r.id;
    if (ev.kind === "session-message") {
      if (own && (r.info.role === "lead" || r.info.role === "manager")) await deliverSessionMessage(r, ev);
      return;
    }
    if (ev.kind === "session-renamed") {
      if (!own) return;
      const name = ev.data.name as string;
      r.knownName = name;
      pi.setSessionName(name);
      ctxOf.current?.ui.notify(`cpd: session renamed to "${name}"`);
      return;
    }
    if (!r.info.planLead) {
      if (ev.kind === "attempt-nudge" && own) pi.sendMessage(eventMessage(ev, r.id, ev.data.prompt as string), FOLLOW_UP(true));
      return;
    }
    const rule = LEAD_MESSAGES[ev.kind];
    if (rule) {
      pi.sendMessage(eventMessage(ev, r.id), { triggerTurn: rule.turn, deliverAs: rule.deliverAs });
      if (ev.kind === "escalation" && r.facts.open(`escalation:${ev.data.escalation as string}`, QUESTION_LABEL)) refresh(r);
      return;
    }
    if (ev.kind === "escalation-answered" || ev.kind === "escalation-expired") {
      if (r.facts.close(`escalation:${ev.data.escalation as string}`)) refresh(r);
      return;
    }
    if (ev.kind === "plan-review" && ev.data.status === "pending") {
      supersede(r, ev.data.review as string);
      void openReview(r, ev.data.review as string, "event")
        .catch((e) => deliver(`plan review ${ev.data.review as string}`, e))
        .finally(() => refresh(r));
    }
    if (ev.kind === "plan-review" && (ev.data.status === "approved" || ev.data.status === "rejected")) {
      const entry = r.reviews.get(ev.data.review as string);
      if (entry?.status === "pending") void decidedElsewhere(r, entry, ev).catch((e) => deliver(`plan review ${entry.review}`, e));
    }
  }

  /** §15.3 `session-message`: for every Lead and a Manager; a turn only when `data.wake` is true (hs-d209, D3). The sender's name is read once, `cpd session show <from>`; the id stands in when it cannot be read. */
  async function deliverSessionMessage(r: Run, ev: CpdEvent): Promise<void> {
    const from = typeof ev.data.from === "string" ? ev.data.from : null;
    let sender: { id: string; name: string | null } | null = null;
    if (from !== null) {
      sender = { id: from, name: null };
      try {
        const shown = (await call(["session", "show", from])).session;
        if (isObject(shown) && typeof shown.name === "string") sender.name = shown.name;
      } catch {
        // an unreadable sender is still named by its id
      }
    }
    if (run !== r) return;
    pi.sendMessage(sessionMessage(ev, r.id, sender), FOLLOW_UP(ev.data.wake === true));
  }

  // ---- Context updates at turn start (§15.7) ----

  /**
   * `before_agent_start` of a Manager only (hs-d209, D1: nothing from the Context is pushed into a Lead's turn): the
   * Context records other Sessions wrote since its cursor, the `Plans:` digest and the fresh `Pending work:` section, as one `cpd-context` message added to the run. Never starts a turn: a failed read is reported once (no turn) and leaves the cursor.
   */
  async function contextUpdates(r: Run, ctx: Ctx): Promise<{ message: Message } | undefined> {
    try {
      const cursor = contextCursorFrom(r.info.registeredEvent, ctx.sessionManager.getEntries(), r.id);
      const events = parseRecordEvents(await call(["events", "list", "--context", r.info.context, "--kind", "context-record", "--since", String(cursor)]));
      const first = Math.min(...events.map((e) => e.seq));
      const records = events.length === 0 ? [] : parseRecords(await call(["context", "show", "--context", r.info.context, "--since", String(first - 1)]));
      const plans = parsePlanEvents(await call(["events", "list", "--context", r.info.context, ...PLAN_KINDS.flatMap((k) => ["--kind", k]), "--since", String(cursor)]));
      const pending = parsePending(await call(["context", "pending", "--context", r.info.context]));
      r.contextFailed = false;
      const message = contextUpdate(r.info.context, r.id, events, records, plans, pending);
      return message === undefined ? undefined : { message };
    } catch (e) {
      if (!r.contextFailed) {
        r.contextFailed = true;
        const err = toCpdError(e);
        const m = errorMessage("context updates", err.code, err.message, err.details);
        pi.sendMessage(m.message, { ...m.options, triggerTurn: false });
      }
      return undefined;
    }
  }

  let updatesRegistered = false;
  /** Registered once, for a Manager only (never a Lead or a Worker); pi has no way to remove a handler. */
  function registerUpdates(): void {
    if (updatesRegistered) return;
    updatesRegistered = true;
    pi.on("before_agent_start", async (_event, ctx) => {
      const r = run;
      return r && r.info.role === "manager" ? contextUpdates(r, ctx) : undefined;
    });
  }

  // ---- the turn-end reminder (§15.8) ----

  /** The Lead's last message of a run, checked at `agent_end` (a failed check adds nothing and is only logged). Registered once; `pi.on` has no unsubscribe, so the run is checked. */
  pi.on("agent_end", async (event) => {
    const r = run;
    if (!r || r.info.role !== "lead" || r.info.plan === null) return;
    r.reminderDue = false;
    const text = lastAssistantText(event?.messages);
    if (text === undefined) return;
    const plan = r.info.plan;
    try {
      const due = needsReminder(text, planKeys(await call(["plan", "show", "--plan", plan])));
      if (run !== r || !due) return;
      r.reminderDue = true;
      registerReminder();
    } catch (e) {
      r.log(`reminder: check failed: ${toCpdError(e).message}`);
    }
  });

  let reminderRegistered = false;
  /**
   * Adds the reminder to the run that is starting: a hidden message (`display: false`), at most once, never a turn of its own.
   * Registered the first time a reminder is due, not before: `pi.on` has no unsubscribe, and a Lead with nothing due (a Manager, a Worker, nearly every Lead)
   * has no `before_agent_start` handler at all, so §15.7's "a Lead gets nothing" stays true of the handler list too.
   */
  function registerReminder(): void {
    if (reminderRegistered) return;
    reminderRegistered = true;
    pi.on("before_agent_start", () => {
      const r = run;
      if (!r || r.info.role !== "lead" || !r.reminderDue) return undefined;
      r.reminderDue = false;
      return { message: { customType: REMINDER_TYPE, content: REMINDER_TEXT, display: false, details: { rule: "hs-d239" } } };
    });
  }

  // ---- the Plan review through plannotator (§15.5) ----

  const runDir = (plan: string) => join(env.CPD_HOME || join(homedir(), ".cpd"), "run", plan);
  const planFile = (plan: string, review: string) => join(runDir(plan), `plan-${review}.md`);
  const feedbackFile = (plan: string, review: string) => join(runDir(plan), `feedback-${review}.md`);

  /** §15.5 "Latest only": a pending request for `review` closes the reasons of this session's older reviews. It does not report: the caller refreshes once the request is handled, so the Lead does not flicker to `idle` between the two reviews. */
  function supersede(r: Run, review: string): void {
    for (const known of r.reviews.keys()) if (reviewNumber(known) < reviewNumber(review)) r.facts.close(`plan-review:${known}`);
  }

  /** Opens a pending review in plannotator. Errors are delivered here; nothing is written to the session on failure. */
  async function openReview(r: Run, review: string, via: "event" | "tool", path?: string): Promise<void> {
    const plan = r.info.plan as string;
    const known = r.reviews.get(review);
    if (known && (via === "event" || known.status === "pending")) return;
    if (r.opening.has(review)) return;
    r.opening.add(review);
    try {
      const latest = (await call(["plan", "show", "--plan", plan])).review;
      if (!isObject(latest) || latest.id !== review || latest.status === "approved" || latest.status === "rejected") return;
      const planFilePath = path ?? planFile(plan, review);
      let planContent: string;
      try {
        planContent = readFileSync(planFilePath, "utf8");
      } catch (e) {
        throw new CpdError("file_unreadable", `cannot read ${planFilePath}: ${e instanceof Error ? e.message : String(e)}`);
      }
      const plannotatorId = parseOpened(await request(pi, "plan-review", { planContent, planFilePath }, timing.requestTimeoutMs));
      if (run !== r) return;
      const entry: ReviewEntry = { plan, review, plannotator_id: plannotatorId, status: "pending" };
      r.reviews.set(review, entry);
      pi.appendEntry(ENTRY_TYPE, entry);
      r.facts.open(`plan-review:${review}`, REVIEW_LABEL);
      refresh(r);
    } catch (e) {
      deliver(`plan review ${review}`, e);
    } finally {
      r.opening.delete(review);
    }
  }

  const tell = (r: Run, content: string, details: Record<string, unknown>) =>
    pi.sendMessage({ customType: DECISION_TYPE, content, display: true, details }, FOLLOW_UP(true));

  /** Appends the settled entry, closes the blocking reason and tells the Lead (§15.5 "Deciding" steps 2 and 3). */
  function settle(r: Run, entry: ReviewEntry, verdict: boolean, text: string | undefined, current: boolean, elsewhere: boolean): void {
    const done: ReviewEntry = { ...entry, status: verdict ? "approved" : "rejected" };
    r.reviews.set(entry.review, done);
    pi.appendEntry(ENTRY_TYPE, done);
    if (r.facts.close(`plan-review:${entry.review}`)) refresh(r);
    tell(r, decisionText(entry.review, verdict, text, current, elsewhere), {
      plan: entry.plan,
      review: entry.review,
      plannotator_id: entry.plannotator_id,
      approved: verdict,
      current,
      ...(text !== undefined ? { feedback: text } : {}),
      ...(elsewhere ? { decided_elsewhere: true } : {}),
    });
  }

  /**
   * §15.5 "Deciding elsewhere": a `plan-review` Event with status `approved` or `rejected` for a pending entry means a caller other than
   * plannotator decided it. cpd already has the decision, so there is no `plan decide`. A failure leaves the entry pending.
   */
  async function decidedElsewhere(r: Run, entry: ReviewEntry, ev: CpdEvent): Promise<void> {
    if (entry.status !== "pending" || r.deciding.has(entry.review)) return;
    r.deciding.add(entry.review);
    try {
      const verdict = ev.data.status === "approved";
      const plan = r.info.plan as string;
      const review = (await call(["plan", "show", "--plan", plan])).review;
      const shown = isObject(review) && review.id === entry.review ? review : undefined;
      const current = shown !== undefined && shown.current !== false;
      const feedback = typeof ev.data.feedback === "string" ? ev.data.feedback : shown !== undefined && typeof shown.feedback === "string" ? shown.feedback : undefined;
      if (run !== r || r.reviews.get(entry.review)?.status !== "pending") return;
      settle(r, entry, verdict, feedback, current, true);
    } finally {
      r.deciding.delete(entry.review);
    }
  }

  /** Records the User's decision with cpd, then tells the Lead. A failure other than `review_decided` leaves the entry pending. */
  async function decide(r: Run, entry: ReviewEntry, approved: boolean, feedback: string | undefined): Promise<void> {
    if (entry.status !== "pending" || r.deciding.has(entry.review)) return;
    r.deciding.add(entry.review);
    const plan = r.info.plan as string;
    let verdict = approved;
    let text = feedback;
    let current = true;
    try {
      const args = ["plan", "decide", entry.review, approved ? "--approve" : "--reject"];
      if (feedback !== undefined && feedback !== "") {
        const file = feedbackFile(plan, entry.review);
        mkdirSync(dirname(file), { recursive: true });
        writeFileSync(file, feedback);
        args.push("--feedback-file", file);
      }
      args.push("--plan", plan);
      try {
        const out = await call(args);
        current = out.current !== false;
      } catch (e) {
        if (toCpdError(e).code !== "review_decided") throw e;
        // Another caller decided it: continue with that decision.
        const review = (await call(["plan", "show", "--plan", plan])).review;
        if (isObject(review) && review.id === entry.review && (review.status === "approved" || review.status === "rejected")) {
          verdict = review.status === "approved";
          text = typeof review.feedback === "string" ? review.feedback : undefined;
          current = review.current !== false;
        } else {
          current = false; // a newer review exists: this decision no longer counts
        }
      }
      if (run !== r) return;
      settle(r, entry, verdict, text, current, false);
    } catch (e) {
      deliver(`plan decide ${entry.review}`, e);
    } finally {
      r.deciding.delete(entry.review);
    }
  }

  /** §15.5 "Recovery": `review-status` every `pollMs` until the review is not pending in plannotator. */
  async function recover(r: Run, review: string): Promise<void> {
    let reported = false;
    for (;;) {
      const entry = r.reviews.get(review);
      if (run !== r || !entry || entry.status !== "pending") return;
      try {
        const status = parseStatus(await request(pi, "review-status", { reviewId: entry.plannotator_id }, timing.requestTimeoutMs));
        reported = false;
        if (run !== r || r.reviews.get(review)?.status !== "pending") return;
        if (status.status === "completed") return await decide(r, entry, status.approved, status.feedback);
        if (status.status === "missing") {
          const lost: ReviewEntry = { ...entry, status: "lost" };
          r.reviews.set(review, lost);
          pi.appendEntry(ENTRY_TYPE, lost);
          if (r.facts.close(`plan-review:${review}`)) refresh(r);
          tell(r, lostText(review), { plan: entry.plan, review, plannotator_id: entry.plannotator_id, lost: true });
          return;
        }
      } catch (e) {
        if (!reported) deliver(`plan review ${review}`, e);
        reported = true;
      }
      try {
        await sleep(timing.pollMs, undefined, { signal: r.abort.signal });
      } catch {
        return; // shut down
      }
    }
  }

  function registerTool(): void {
    if (toolRegistered) return;
    toolRegistered = true;
    pi.registerTool({
      name: "cpd_plan_review",
      label: "cpd plan review",
      description:
        "Ask the User to review and approve the current cpd Plan (its draft Jobs) in plannotator. Returns at once; the decision arrives later as a cpd message. Call it once per revision.",
      parameters: { type: "object", properties: {}, additionalProperties: false },
      exposure: "model-only",
      async execute() {
        const r = run;
        if (!r || !r.info.planLead) throw new CpdError("not_a_lead", "cpd_plan_review is only available in a Plan Lead's session");
        const out = await call(["plan", "review", "--plan", env.CPD_PLAN || (r.info.plan as string)]);
        const review = isObject(out.review) && typeof out.review.id === "string" ? out.review.id : undefined;
        const already = out.already === true;
        if (review !== undefined && already) {
          const known = r.reviews.get(review);
          if (known?.status !== "pending") await openReview(r, review, "tool", typeof out.path === "string" ? out.path : undefined);
        }
        const next = typeof out.next === "string" ? out.next : "The User reviews the plan in plannotator. Stop and wait for the decision message.";
        return { content: [{ type: "text" as const, text: next }], details: { review, already } };
      },
    });
  }

  // ---- pi's lifecycle ----

  pi.on("session_start", async (event, ctx) => {
    try {
      await start(ctx, typeof event?.reason === "string" ? event.reason : "unknown");
    } catch (e) {
      deliver("start-up", e);
    }
  });

  pi.on("session_shutdown", async () => {
    try {
      await stop();
    } catch (e) {
      deliver("shutdown", e);
    }
  });

  // hs-d278: who started the run. `pi.on` has no unsubscribe, so the run is checked. Returns nothing: it adds no message.
  pi.on("before_agent_start", function markReply(event) {
    const r = run;
    if (r) r.facts.reply = startsReply(event);
  });

  pi.on("input", (event) => {
    const r = run;
    if (r && r.facts.active && isTyped(event)) r.facts.reply = true; // typing during a run (steer or follow-up) makes it a reply run
  });

  pi.on("agent_start", () => {
    const r = run;
    if (!r) return;
    r.facts.active = true;
    refresh(r);
  });

  // pi's dialogs (`ctx.ui.select/confirm/input/editor/custom`): `pi.on` has no unsubscribe, so the run is checked.
  const onPrompt = (active: boolean) => {
    const r = run;
    if (!r) return;
    if (r.facts.prompt(active)) refresh(r);
  };
  pi.on("ui_prompt_start", () => onPrompt(true));
  pi.on("ui_prompt_end", () => onPrompt(false));

  pi.on("agent_settled", (_event, ctx) => {
    const r = run;
    if (!r) return;
    if (ctx.isIdle()) r.facts.active = false;
    refresh(r);
    r.reload?.nudge(); // a busy Lead's idle gaps can be shorter than the poll interval (§15.6 "Retries")
  });

  pi.on("session_info_changed", () => {
    const r = run;
    if (!r) return;
    const name = pi.getSessionName();
    if (name === undefined || name === "" || name === r.knownName) return;
    r.knownName = name;
    void call(["session", "rename", name, "--session", r.id]).catch((e) => deliver("session rename", e));
  });
}
