import assert from "node:assert/strict";
import { test } from "node:test";
import type { Host } from "../lib/host.ts";
import { clearMentions, refreshMentions } from "../lib/mentions-refresh.ts";
import type { Run } from "../lib/run.ts";

/** commands.md §15.14 (hs-d544): the mentions refresh lists live and archived Leads and Managers with `session list --archived` once (it already lists both; clap refuses it twice), and a failure that repeats the same way is shown once. */
const run = { info: { role: "lead" } } as unknown as Run;

type Reply = (args: string[]) => Promise<unknown>;

function fakeHost(reply: Reply) {
  const calls: string[][] = [];
  const delivered: Array<{ what: string; e: unknown }> = [];
  const host = {
    run,
    ctx: undefined,
    call: (args: string[]) => {
      calls.push(args);
      return reply(args);
    },
    deliver: (what: string, e: unknown) => {
      delivered.push({ what, e });
    },
  } as unknown as Host;
  return { host, calls, delivered };
}

const ok: Reply = async (args) => ({ ok: true, sessions: [], plans: [], contexts: [], ...{ _: args[0] } });
const failing = (message: string): Reply => async () => {
  throw new Error(message);
};

test("refresh lists lead and manager sessions with --archived once", async (t) => {
  t.after(clearMentions);
  const f = fakeHost(ok);
  await refreshMentions(f.host, run);
  const list = f.calls.find((a) => a[0] === "session" && a[1] === "list");
  assert.ok(list, "a session list call was made");
  assert.equal(list.filter((a) => a === "--archived").length, 1, `argv: ${list.join(" ")}`);
  assert.deepEqual(list.filter((a) => a === "--role").length, 2);
  assert.deepEqual(list.filter((_, i) => list[i - 1] === "--role").sort(), ["lead", "manager"]);
  assert.deepEqual(f.delivered, []);
});

test("a failure repeated the same way is delivered once", async (t) => {
  t.after(clearMentions);
  const f = fakeHost(failing("repeat: store lock_timeout"));
  await refreshMentions(f.host, run);
  await refreshMentions(f.host, run);
  assert.equal(f.delivered.length, 1);
  assert.equal(f.delivered[0]!.what, "mentions");
});

test("a different failure or a success re-arms the delivery", async (t) => {
  t.after(clearMentions);
  let reply: Reply = failing("rearm: same");
  const f = fakeHost((a) => reply(a));
  await refreshMentions(f.host, run);
  await refreshMentions(f.host, run);
  reply = ok;
  await refreshMentions(f.host, run);
  reply = failing("rearm: same");
  await refreshMentions(f.host, run);
  await refreshMentions(f.host, run);
  assert.equal(f.delivered.length, 2, "same error, a success, the same error: twice");

  const g = fakeHost((a) => reply(a));
  reply = failing("rearm: A");
  await refreshMentions(g.host, run);
  await refreshMentions(g.host, run);
  reply = failing("rearm: B");
  await refreshMentions(g.host, run);
  await refreshMentions(g.host, run);
  assert.equal(g.delivered.length, 2, "error A then error B: twice");
});
