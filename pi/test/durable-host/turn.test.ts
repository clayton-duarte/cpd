import assert from "node:assert/strict";
import { test } from "node:test";
import { CpdError } from "../../lib/cpd.ts";
import { REMINDER_TEXT } from "../../lib/reminder.ts";
import { newTurnState, prepareTurn, reminderFor, reportHostError, type Seat, type StoredEntry, type TurnPort, type Write } from "../../durable-host/turn.ts";
import { ID } from "../answers.ts";

type Answer = Record<string, unknown> | Error;
type Fake = { port: TurnPort; calls: string[][]; writes: Write[]; logs: string[] };

const key = (args: readonly string[]) => args.slice(0, 2).join("-");
const seat = (over: Partial<Seat> = {}): Seat => ({ id: ID, role: "lead", context: "ctx", plan: "p1", registeredEvent: 100, byUser: false, ...over });

/** A fake `Deps.cpd` answering by `<a>-<b>` (answers.ts's key), a recording `submit` and a fixed entry list. An unlisted command answers `{ ok: true }`. */
function fake(s: Seat, answers: Record<string, Answer | (() => Answer)> = {}, entries: StoredEntry[] = []): Fake {
  const calls: string[][] = [];
  const writes: Write[] = [];
  const logs: string[] = [];
  const port: TurnPort = {
    seat: s,
    cpd: async (args) => {
      calls.push([...args]);
      const a = answers[key(args)];
      const v = typeof a === "function" ? a() : a;
      if (v instanceof Error) throw v;
      return v ?? { ok: true };
    },
    submit: async (w) => {
      writes.push(w);
      return {};
    },
    entries: async () => entries,
    log: (l) => logs.push(l),
  };
  return { port, calls, writes, logs };
}

const FIRST = { ok: true, text: "Build the thing.", event: 120 };
const reads = (f: Fake, a: string, b: string) => f.calls.filter((c) => c[0] === a && c[1] === b);
const hasText = (w: Write, text: string) => JSON.stringify(w.entry.model).includes(text);
const withModel = (w: Write) => w.entry.model.length > 0;
const news = { "events-list": { ok: true, events: [] }, "board-take": { ok: true, taken: [{ at: "2026-10-14T21:00:00Z", from_session: null, text: "Mind the schema." }] } };

test("a by-user Session's first message is stored once before any input", async () => {
  const f = fake(seat({ byUser: true }), { "session-first-message": FIRST });
  const state = newTurnState();
  await prepareTurn(f.port, state);
  await prepareTurn(f.port, state);
  const stored = f.writes.filter((w) => w.entry.kind === "cpd-first-message");
  assert.equal(stored.length, 1);
  assert.equal(stored[0]!.requestId, `first-message:${ID}`);
  assert.deepEqual(stored[0]!.entry.data, { event: 120, delivered_to: ID });
  assert.ok(withModel(stored[0]!) && hasText(stored[0]!, "Build the thing."));
  assert.deepEqual(reads(f, "session", "first-message")[0], ["session", "first-message", "--session", ID]);
});

test("a stored first message is not stored again after a restart", async () => {
  const stored: StoredEntry[] = [{ kind: "cpd-first-message", data: { event: 120, delivered_to: ID }, model: [{ role: "user", content: "Build the thing." }] }];
  const f = fake(seat({ byUser: true }), { "session-first-message": FIRST }, stored);
  await prepareTurn(f.port, newTurnState());
  assert.equal(reads(f, "session", "first-message").length, 0);
  assert.equal(f.writes.filter((w) => w.entry.kind === "cpd-first-message").length, 0);
});

test("a failing cpd session first-message is reported once and tried again at the next turn", async () => {
  let n = 0;
  const f = fake(seat({ byUser: true }), { "session-first-message": () => (++n <= 2 ? new CpdError("busy", "store busy") : FIRST), "session-report": { ok: true } });
  const state = newTurnState();
  await prepareTurn(f.port, state);
  assert.equal(f.writes.filter((w) => w.entry.kind === "cpd-first-message").length, 0);
  await prepareTurn(f.port, state);
  const reports = reads(f, "session", "report");
  assert.equal(reports.length, 1);
  assert.deepEqual(reports[0]!.slice(0, 7), ["session", "report", "--what", "first message", "--code", "busy", "--message"]);
  assert.equal(reads(f, "session", "first-message").length, 2);
  await prepareTurn(f.port, state);
  assert.equal(reads(f, "session", "first-message").length, 3);
  assert.equal(f.writes.filter((w) => w.entry.kind === "cpd-first-message").length, 1);
  assert.equal(reads(f, "session", "report").length, 1);
});

test("context updates are stored before the input of a turn when cpd has news", async () => {
  const f = fake(seat(), news);
  await prepareTurn(f.port, newTurnState());
  const updates = f.writes.filter((w) => w.entry.kind === "cpd-context");
  assert.equal(updates.length, 1);
  const u = updates[0]!;
  assert.ok(u.entry.model.length > 0 && hasText(u, "Mind the schema."));
  assert.equal(u.entry.data.delivered_to, ID);
  assert.ok(Number.isSafeInteger(u.entry.data.event));
  assert.ok(Array.isArray(u.entry.data.records) && typeof u.entry.data.summary === "string");
  const board = f.calls.findIndex((c) => key(c) === "board-take");
  assert.ok(board >= 0 && f.calls.slice(board + 1).every((c) => key(c) !== "events-list"), "the board posts are taken last");
});

test("no context update is stored when cpd has no news", async () => {
  const f = fake(seat(), { "events-list": { ok: true, events: [] }, "board-take": { ok: true, taken: [] }, "plan-show": { ok: true, jobs: [], workflows: [] } });
  await prepareTurn(f.port, newTurnState());
  assert.equal(f.writes.length, 0);
  assert.ok(reads(f, "board", "take").length === 1, "cpd was asked");
});

test("the context update cursor is read from the stored entries after a restart", async () => {
  const stored: StoredEntry[] = [
    { kind: "cpd-context", data: { event: 150, delivered_to: ID }, model: [{}] },
    { kind: "cpd-context", data: { event: 400, delivered_to: "the-source-session" }, model: [{}] },
    { kind: "cpd-event", data: { id: 130, kind: "session-message", delivered_to: ID } },
  ];
  const f = fake(seat(), news, stored);
  await prepareTurn(f.port, newTurnState());
  const lists = f.calls.filter((c) => key(c) === "events-list");
  assert.ok(lists.length > 0);
  for (const c of lists) assert.equal(c[c.indexOf("--since") + 1], "150", `--since of ${c.join(" ")}`);
});

test("the turn-end reminder continues a Plan Lead's run once when its last answer needs one", async () => {
  const f = fake(seat(), { "plan-show": { ok: true, jobs: [{ key: "j1" }], workflows: [{ key: "pr1" }] } });
  const state = newTurnState();
  const messages = [{ role: "assistant", content: [{ type: "text", text: "Merged. The next steps are a cleanup." }] }];
  assert.equal(await reminderFor(messages, f.port, state), REMINDER_TEXT);
  assert.deepEqual(reads(f, "plan", "show")[0], ["plan", "show", "--plan", "p1"]);
  assert.equal(await reminderFor(messages, f.port, state), undefined);
  assert.equal(f.writes.length, 0);
});

test("a Session with no Plan gets no reminder", async () => {
  const f = fake(seat({ plan: null }), { "plan-show": { ok: true, jobs: [], workflows: [] } });
  const messages = [{ role: "assistant", content: "The next steps are a cleanup." }];
  assert.equal(await reminderFor(messages, f.port, newTurnState()), undefined);
  assert.equal(f.calls.length, 0);
});

test("a host error of a Lead is reported with cpd session report", async () => {
  const f = fake(seat(), { "session-report": { ok: true } });
  const state = newTurnState();
  const err = { what: "context updates", code: "busy", message: "store busy", details: { try: 2 } };
  await reportHostError(err, f.port, state);
  await reportHostError(err, f.port, state);
  assert.deepEqual(f.calls, [["session", "report", "--what", "context updates", "--code", "busy", "--message", "store busy", "--details", JSON.stringify({ try: 2 }), "--session", ID]]);
  assert.equal(f.writes.length, 0);
  await reportHostError({ ...err, code: "bad_output" }, f.port, state);
  assert.equal(f.calls.length, 2);
});

test("a host error of a Manager is stored as a silent chat entry", async () => {
  const f = fake(seat({ role: "manager", plan: null }));
  const state = newTurnState();
  const err = { what: "context updates", code: "busy", message: "store busy" };
  await reportHostError(err, f.port, state);
  await reportHostError(err, f.port, state);
  assert.equal(f.calls.length, 0);
  assert.equal(f.writes.length, 1);
  const e = f.writes[0]!.entry;
  assert.equal(e.kind, "cpd-error");
  assert.deepEqual(e.data, { kind: "error", code: "busy", what: "context updates" });
  assert.deepEqual(e.model, []);
  assert.ok(JSON.stringify(f.writes[0]).includes("cpd adapter: context updates: busy: store busy"));
});
