import { toCpdError } from "./cpd.ts";
import { errorMessage } from "./delivery.ts";
import type { Host } from "./host.ts";
import type { Ctx, Message } from "./pi.ts";
import { lastAssistantText, needsReminder, planKeys, REMINDER_TEXT, REMINDER_TYPE } from "./reminder.ts";
import type { Run } from "./run.ts";
import { contextCursorFrom, contextUpdate, parsePending, parsePlanEvents, parseRecordEvents, parseRecords, PLAN_KINDS } from "./updates.ts";

// ---- Context updates at turn start (§15.7) ----

/**
 * `before_agent_start` of a Manager only (nothing from the Context is pushed into a Lead's turn): the
 * Context records other Sessions wrote since its cursor, the `Plans:` digest and the fresh `Pending work:` section, as one `cpd-context` message added to the run. Never starts a turn: a failed read is reported once (no turn) and leaves the cursor.
 */
async function contextUpdates(host: Host, r: Run, ctx: Ctx): Promise<{ message: Message } | undefined> {
  const { call } = host;
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
      host.pi.sendMessage(m.message, { ...m.options, triggerTurn: false });
    }
    return undefined;
  }
}

/** Registered once, for a Manager only (never a Lead or a Worker); pi has no way to remove a handler. */
export function registerUpdates(host: Host): void {
  if (host.registered.updates) return;
  host.registered.updates = true;
  host.pi.on("before_agent_start", async (_event, ctx) => {
    const r = host.run;
    return r && r.info.role === "manager" ? contextUpdates(host, r, ctx) : undefined;
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
