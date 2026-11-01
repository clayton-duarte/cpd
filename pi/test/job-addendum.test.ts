import assert from "node:assert/strict";
import { test } from "node:test";
import { KINDS } from "../lib/event.ts";
import { ev, Fixture, ID, planShow, shown } from "./fake.ts";

// contract §3.4, commands.md §15.3: a `job-addendum` of this Session is a steered message with a turn for a Worker; a Plan Lead gets nothing.

const addendum = (id: number, session: string | null = ID) => ev(id, "job-addendum", { session, job: "j", data: { addendum: 1 } });
const marker = (id: number) => ev(id, "attempt-nudge", { session: ID, data: { prompt: "Submit your work.", turns: 1 } });

async function worker(t: { after(fn: () => void): void }) {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown({ role: "worker", plan_lead: false }));
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  return f;
}

test("a Worker is delivered a job-addendum for its Session as a steered message that triggers a turn", async (t) => {
  const f = await worker(t);
  f.emit(addendum(101));
  await f.drain(marker);

  const sent = f.pi.delivered();
  assert.equal(sent.length, 1);
  assert.equal(sent[0]!.message.content, "text of job-addendum 101");
  assert.deepEqual(sent[0]!.options, { triggerTurn: true, deliverAs: "steer" });
  assert.equal((sent[0]!.message.details as any).id, 101);
});

test("a Worker is delivered a job-addendum id once when the follower prints it twice", async (t) => {
  const f = await worker(t);
  const a = addendum(101);
  f.emit(a, a);
  await f.drain(marker);

  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [101]);
});

test("a Worker ignores a job-addendum addressed to another Session or to none", async (t) => {
  const f = await worker(t);
  f.emit(addendum(101, "another-session"), addendum(102, null), addendum(103));
  await f.drain(marker);

  assert.deepEqual(f.pi.delivered().map((s) => (s.message.details as any).id), [103], "only the Event for this Session is delivered");
});

test("a Plan Lead gets no message and no turn for a job-addendum of its Plan", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  f.respond("plan-show", planShow());
  f.respond("session-show", shown());
  f.load();
  await f.pi.fire("session_start");
  await f.followerStarted();
  assert.ok((KINDS as readonly string[]).includes("job-addendum"), "job-addendum is not in KINDS");
  f.emit(addendum(101, null), addendum(102));
  await f.drain();

  assert.deepEqual(f.pi.delivered(), []);
  assert.equal(f.pi.sent.length, 0);
});
