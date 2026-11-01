import { toCpdError } from "../lib/cpd.ts";
import { lastAssistantText, needsReminder, planKeys, REMINDER_TEXT } from "../lib/reminder.ts";
import { closedPrLines, contextUpdate, countsOf, type ErrorEvent, leadUpdate, parseErrorEvents, parsePending, parsePlanEvents, parsePrOpened, parseRecordEvents, parseRecords, parseTaken, PLAN_KINDS, summaryLine } from "../lib/updates.ts";
import type { Message } from "../lib/pi.ts";
import { cursorFrom } from "./cursor.ts";
/** Contract §6.13 "Delivery (P1.4)", steps 3 to 6: what the host stores before an input that starts a turn, the turn-end reminder and the host errors. Effects go through `TurnPort`. */

/** An entry as the conversation stores it (`Conversation.entries`): its `kind`, `data` and the model messages it carries. */
export type StoredEntry = { kind: string; data?: Record<string, unknown>; model?: readonly unknown[] };

/** One `conversation.submit({ type: "write", ... })`. `model: []` is a silent write that changes nothing the model sees. `text` is the line a chat shows for a silent entry (an `EntryDraft` has no text field of its own). */
export type Write = { type: "write"; entry: { kind: string; data: Record<string, unknown>; model: readonly unknown[] }; requestId?: string; text?: string };

/** The Session the host runs, as `cpd session show` gives it. `byUser` is true when the User created it (it has a first message). */
export type Seat = { id: string; role: "lead" | "manager"; context: string; plan: string | null; registeredEvent: number; byUser: boolean };

/** Every effect of this module. */
export type TurnPort = {
  seat: Seat;
  cpd(args: readonly string[]): Promise<Record<string, unknown>>;
  submit(write: Write): Promise<unknown>;
  entries(): Promise<readonly StoredEntry[]>;
  log(line: string): void;
};

/** What lives for one run of the host: the "once" memories of §6.13. */

export type TurnState = { firstDone: boolean; firstFailed: boolean; firstEvent: number; contextFailed: boolean; lastError: string | undefined; reminded: boolean };

export const newTurnState = (): TurnState => ({ firstDone: false, firstFailed: false, firstEvent: 0, contextFailed: false, lastError: undefined, reminded: false });

/** A host error: `what` and `code` are the memory's key. */
export type HostError = { what: string; code: string; message: string; details?: Record<string, unknown> };

const toWrite = (m: Message, requestId?: string): Write => ({
  type: "write",
  entry: { kind: m.customType, data: m.details as Record<string, unknown>, model: [{ role: "user", content: m.content }] },
  ...(requestId === undefined ? {} : { requestId }),
});

const reportsOnce = (state: TurnState, what: string, code: string) => {
  const key = `${what}:${code}`;
  if (state.lastError === key) return false;
  state.lastError = key;
  return true;
};

/** Before an input that starts a turn: stores the first message while it is pending, then the context update, each as one write. A failed read is reported once and never throws. */
export async function prepareTurn(port: TurnPort, state: TurnState): Promise<void> {
  const { seat } = port;
  const fail = async (flag: "firstFailed" | "contextFailed", what: string, e: unknown) => {
    if (state[flag]) return;
    state[flag] = true;
    const err = toCpdError(e);
    await reportHostError({ what, code: err.code, message: err.message, ...(err.details === undefined ? {} : { details: err.details as Record<string, unknown> }) }, port, state);
  };
  let stored: readonly StoredEntry[] = [];
  try {
    stored = await port.entries();
  } catch (e) {
    port.log(`entries: ${toCpdError(e).message}`);
  }
  // The first message goes first and alone: while it is pending, or stored by this very call, the context update waits for the next turn (the first message's event is the cursor then).
  if (seat.byUser && !state.firstDone) {
    if (stored.some((e) => e.kind === "cpd-first-message" && e.data?.delivered_to === seat.id)) state.firstDone = true;
    else {
      try {
        const out = await port.cpd(["session", "first-message", "--session", seat.id]);
        if (typeof out.text !== "string" || !Number.isSafeInteger(out.event)) throw new Error("cpd session first-message: the output lacks text or event");
        const event = out.event as number;
        await port.submit(toWrite({ customType: "cpd-first-message", content: out.text, display: false, details: { event, delivered_to: seat.id } }, `first-message:${seat.id}`));
        state.firstDone = true;
        state.firstFailed = false;
        state.firstEvent = Math.max(state.firstEvent, event);
      } catch (e) {
        await fail("firstFailed", "first message", e);
      }
      state.reminded = false;
      return;
    }
  }
  try {
    const message = await contextMessage(port, state, stored);
    state.contextFailed = false;
    if (message !== undefined) {
      const details = message.details as Record<string, unknown>;
      const counts = countsOf(details);
      await port.submit(toWrite({ ...message, details: { ...details, summary: typeof details.summary === "string" ? details.summary : summaryLine(counts ?? { records: [], plan_events: 0, plans_pending: 0 }) } }));
    }
  } catch (e) {
    await fail("contextFailed", "context updates", e);
  }
  state.reminded = false;
}

/** The context update of contract §6.13 step 4 (the reads of pi/lib/turns.ts); the board posts are taken last, so a failed read loses none. */
async function contextMessage(port: TurnPort, state: TurnState, stored: readonly StoredEntry[]): Promise<Message | undefined> {
  const { seat, cpd } = port;
  const context = seat.context;
  const cursor = Math.max(state.firstEvent, cursorFrom(seat.registeredEvent, stored, seat.id));
  const since = String(cursor);
  const events = parseRecordEvents(await cpd(["events", "list", "--context", context, "--kind", "context-record", "--since", since]));
  const first = Math.min(...events.map((e) => e.seq));
  const records = events.length === 0 ? [] : parseRecords(await cpd(["context", "show", "--context", context, "--since", String(first - 1)]));
  if (seat.role === "lead") {
    const plan = seat.plan;
    const workflows = plan === null ? [] : parsePrOpened(await cpd(["events", "list", "--context", context, "--kind", "pr-opened", "--since", since])).filter((p) => p.plan === plan);
    let standing: string[] = [];
    if (plan !== null) {
      try {
        standing = closedPrLines(await cpd(["plan", "show", "--plan", plan]));
      } catch (e) {
        const err = toCpdError(e);
        if (err.code === "usage") throw err;
        standing = [`could not read the Plan's Workflows: ${err.message.split("\n")[0]}`];
      }
    }
    return leadUpdate(context, seat.id, events, records, workflows, parseTaken(await cpd(["board", "take", "--session", seat.id])), standing);
  }
  const plans = parsePlanEvents(await cpd(["events", "list", "--context", context, ...PLAN_KINDS.flatMap((k) => ["--kind", k]), "--since", since]));
  const pending = parsePending(await cpd(["context", "pending", "--context", context]));
  const errors = new Map<number, ErrorEvent>();
  for (const scope of [["--context", context], []]) for (const e of parseErrorEvents(await cpd(["events", "list", ...scope, "--kind", "error", "--since", since]))) errors.set(e.event, e);
  const posts = parseTaken(await cpd(["board", "take", "--session", seat.id]));
  return contextUpdate(context, seat.id, events, records, plans, pending, { posts, errors: [...errors.values()], cursor });
}

/**
 * At a run's end (`onYield`): the text to continue the run with, or undefined. Plan Leads only, once per run. `messages` is the run's messages.
 * Checked in pi-durable 1.3.0 (README "Hooks", dist/harness/types.d.ts `GenerationHooks`): `onYield(answer, api, context)` gets the final AssistantMessage and
 * `{ continue: UserInput }` appends a user message and continues the run, so the caller passes `[answer]` as `messages`.
 */
export async function reminderFor(messages: unknown, port: TurnPort, state: TurnState): Promise<string | undefined> {
  const { seat } = port;
  if (state.reminded || seat.role !== "lead" || seat.plan === null) return undefined;
  const text = lastAssistantText(messages);
  if (text === undefined) return undefined;
  try {
    if (!needsReminder(text, planKeys(await port.cpd(["plan", "show", "--plan", seat.plan])))) return undefined;
  } catch (e) {
    port.log(`reminder: check failed: ${toCpdError(e).message}`);
    return undefined;
  }
  state.reminded = true;
  return REMINDER_TEXT;
}

/** A Lead's error is one `cpd session report`; a Manager's one silent `cpd-error` write; once per (`what`, `code`) in a row. */
export async function reportHostError(error: HostError, port: TurnPort, state: TurnState): Promise<void> {
  if (!reportsOnce(state, error.what, error.code)) return;
  const { seat } = port;
  try {
    if (seat.role === "lead") {
      await port.cpd(["session", "report", "--what", error.what, "--code", error.code, "--message", error.message, ...(error.details === undefined ? [] : ["--details", JSON.stringify(error.details)]), "--session", seat.id]);
    } else {
      await port.submit({ type: "write", entry: { kind: "cpd-error", data: { kind: "error", code: error.code, what: error.what }, model: [] }, text: `cpd adapter: ${error.what}: ${error.code}: ${error.message}` });
    }
  } catch (e) {
    port.log(`could not report ${error.what}: ${toCpdError(e).message}`);
  }
}
