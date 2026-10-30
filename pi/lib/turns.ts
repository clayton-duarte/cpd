import { toCpdError } from "./cpd.ts";
import type { Host } from "./host.ts";
import type { Ctx, Message } from "./pi.ts";
import { lastAssistantText, needsReminder, planKeys, REMINDER_TEXT, REMINDER_TYPE } from "./reminder.ts";
import type { Run } from "./run.ts";
import { closedPrLines, contextCursorFrom, contextUpdate, type ErrorEvent, leadUpdate, parseErrorEvents, parsePending, parsePlanEvents, parsePrOpened, parseRecordEvents, parseRecords, parseTaken, PLAN_KINDS } from "./updates.ts";

// ---- Context updates at turn start (§15.7) ----

/** The Event ids and records one read of the Context's records gave. */
async function readRecords(host: Host, r: Run, cursor: number) {
  const { call } = host;
  const events = parseRecordEvents(await call(["events", "list", "--context", r.info.context, "--kind", "context-record", "--since", String(cursor)]));
  const first = Math.min(...events.map((e) => e.seq));
  const records = events.length === 0 ? [] : parseRecords(await call(["context", "show", "--context", r.info.context, "--since", String(first - 1)]));
  return { events, records };
}

/**
 * `before_agent_start` of a Lead or a Manager: one `cpd-context` message added to the run, never a turn. A Lead's is short (new decisions, the board posts, its Plan's opened PRs); a Manager's
 * is the turn-start update (records, board posts, `Plans:`, `Pending work:`, `Errors:`). The board posts are taken last, so a failed read loses none. A failed read is reported once (no turn) and leaves the cursor.
 */
async function contextUpdates(host: Host, r: Run, ctx: Ctx): Promise<{ message: Message } | undefined> {
  const { call } = host;
  const context = r.info.context;
  try {
    const cursor = Math.max(r.firstEvent, contextCursorFrom(r.info.registeredEvent, ctx.sessionManager.getEntries(), r.id));
    const { events, records } = await readRecords(host, r, cursor);
    let message: Message | undefined;
    if (r.info.role === "lead") {
      const plan = r.info.plan;
      const prs = plan === null ? [] : parsePrOpened(await call(["events", "list", "--context", context, "--kind", "pr-opened", "--since", String(cursor)])).filter((p) => p.plan === plan);
      let standing: string[] = [];
      if (plan !== null) {
        try {
          standing = closedPrLines(await call(["plan", "show", plan]));
        } catch (e) {
          standing = [`could not read the Plan's PRs: ${toCpdError(e).message.split("\n")[0]}`];
        }
      }
      message = leadUpdate(context, r.id, events, records, prs, parseTaken(await call(["board", "take", "--session", r.id])), standing);
    } else {
      const plans = parsePlanEvents(await call(["events", "list", "--context", context, ...PLAN_KINDS.flatMap((k) => ["--kind", k]), "--since", String(cursor)]));
      const pending = parsePending(await call(["context", "pending", "--context", context]));
      const errors = new Map<number, ErrorEvent>();
      for (const scope of [["--context", context], []]) for (const e of parseErrorEvents(await call(["events", "list", ...scope, "--kind", "error", "--since", String(cursor)]))) errors.set(e.event, e);
      const posts = parseTaken(await call(["board", "take", "--session", r.id]));
      message = contextUpdate(context, r.id, events, records, plans, pending, { posts, errors: [...errors.values()], cursor });
    }
    r.contextFailed = false;
    return message === undefined ? undefined : { message };
  } catch (e) {
    if (!r.contextFailed) {
      r.contextFailed = true;
      const err = toCpdError(e);
      host.report("context updates", err.code, err.message, err.details);
    }
    return undefined;
  }
}

/** Registered once, for a Lead or a Manager (never a Worker); pi has no way to remove a handler. The role is read at each call. */
export function registerUpdates(host: Host): void {
  if (host.registered.updates) return;
  host.registered.updates = true;
  host.pi.on("before_agent_start", async (_event, ctx) => {
    const r = host.run;
    return r && (r.info.role === "manager" || r.info.role === "lead") ? contextUpdates(host, r, ctx) : undefined;
  });
}

// ---- the turn-end reminder (§15.8) ----

/**
 * Adds the reminder to the run that is starting: a hidden message (`display: false`), at most once, never a turn of its own.
 * Registered the first time a reminder is due, not before: `pi.on` has no unsubscribe, and a Lead with nothing due (a Manager, a Worker, nearly every Lead)
 * has no `before_agent_start` handler at all, so §15.7's "a Lead gets nothing" stays true of the handler list too.
 */
function registerReminder(host: Host): void {
  if (host.registered.reminder) return;
  host.registered.reminder = true;
  host.pi.on("before_agent_start", () => {
    const r = host.run;
    if (!r || r.info.role !== "lead" || !r.reminderDue) return undefined;
    r.reminderDue = false;
    return { message: { customType: REMINDER_TYPE, content: REMINDER_TEXT, display: false, details: { rule: "register-planned-work" } } };
  });
}

/** The Lead's last message of a run, checked at `agent_end` (a failed check adds nothing and is only logged). Registered once; `pi.on` has no unsubscribe, so the run is checked. */
export function registerReminderCheck(host: Host): void {
  host.pi.on("agent_end", async (event) => {
    const r = host.run;
    if (!r || r.info.role !== "lead" || r.info.plan === null) return;
    r.reminderDue = false;
    const text = lastAssistantText(event?.messages);
    if (text === undefined) return;
    const plan = r.info.plan;
    try {
      const due = needsReminder(text, planKeys(await host.call(["plan", "show", "--plan", plan])));
      if (host.run !== r || !due) return;
      r.reminderDue = true;
      registerReminder(host);
    } catch (e) {
      r.log(`reminder: check failed: ${toCpdError(e).message}`);
    }
  });
}
