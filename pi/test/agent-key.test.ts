import assert from "node:assert/strict";
import { test } from "node:test";
import { parseSession } from "../lib/run.ts";
import { Fixture, ID, shown, waitFor } from "./fake.ts";

const U = "aaaaaaaa-1111-4222-8333-444444444444";

async function started(f: Fixture, over: Record<string, unknown>) {
  f.respond("session-show", shown(over));
  f.respond("plan-show", { ok: true, plan: { id: "p1" }, jobs: [], escalations: [], review: null });
  f.load();
  await f.pi.fire("session_start");
  await waitFor("the follower", () => f.follows().length > 0);
}
const renames = (f: Fixture) => f.argvs().filter((a) => a[1] === "rename");

test("a name typed in pi for a Session with a key is put back to the key and runs no rename", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, { name: "ctx:p1", key: "ctx:p1" });
  f.pi.setSessionName("typed");
  await f.pi.fire("session_info_changed", { name: "typed" });
  await f.quiet();
  assert.equal(f.pi.sessionName, "ctx:p1");
  assert.deepEqual(renames(f), []);
});

test("a name typed in pi for a Session without a key still runs session rename", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, { name: "src (fork)", key: null, forked_from: U });
  f.pi.setSessionName("typed");
  await f.pi.fire("session_info_changed", { name: "typed" });
  await f.quiet();
  assert.equal(renames(f).length, 1);
  assert.equal(f.pi.sessionName, "typed");
  assert.equal((parseSession(shown({ name: "src (fork)", key: null, forked_from: U }) as any) as any).key, null, "the adapter reads key: null for a fork");
});

test("a name typed in pi for a Session with a key shows the fixed name notice", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  await started(f, { name: "ctx:p1", key: "ctx:p1" });
  f.pi.setSessionName("typed");
  await f.pi.fire("session_info_changed", { name: "typed" });
  await f.quiet();
  assert.ok(f.pi.notices.includes("\x1b[1mφ\x1b[22m: ctx:p1 is this session's key: its name cannot change."), JSON.stringify(f.pi.notices));
});

const post = (from: string | null, name: string | null) => ({ ok: true, session: ID, taken: [{ id: 1, at: "2026-10-21T15:30:00Z", from_session: from, from_name: name, text: "hello" }] });
async function content(f: Fixture, from: string | null, name: string | null) {
  await started(f, {});
  f.respond("board-take", post(from, name));
  const r = (await f.pi.fireWith("before_agent_start", { messageType: "user", prompt: "go" })) as any[];
  return String(r.find((x) => x !== undefined)?.message?.content);
}

test("a board post reaches the Lead as Board post from the sender name and id", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  assert.ok((await content(f, U, "cpd:plan-x")).includes(`Board post from cpd:plan-x (${U})`));
});

test("a board post from the User reaches the Lead as Board post from the User", async (t) => {
  const f = new Fixture();
  t.after(() => f.close());
  assert.ok((await content(f, null, null)).includes("Board post from the User"));
});

test("session show without a key field is bad output", () => {
  const { key: _k, ...session } = (shown() as any).session;
  assert.throws(() => parseSession({ ...shown(), session }), { code: "bad_output" });
});
