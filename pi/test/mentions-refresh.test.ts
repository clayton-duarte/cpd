import assert from "node:assert/strict";
import { test } from "node:test";
import { createHost, type Host } from "../lib/host.ts";
import { clearMentions, refreshMentions } from "../lib/mentions-refresh.ts";
import { Run } from "../lib/run.ts";
import { FakePi } from "./fake.ts";
import { ID } from "./answers.ts";

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

test("a mentions failure repeated after a success is not shown again", async (t) => {
  t.after(clearMentions);
  // The real `host.deliver`: the §15.1 rule (the same what and code is shown once) is what this pins.
  const pi = new FakePi();
  const host = createHost(pi, {});
  const live = new Run({
    id: ID,
    info: { role: "lead", context: "c", plan: "p1", planLead: true, registeredEvent: 0, name: null, byUser: false },
    facts: {} as never,
    reporter: {} as never,
    abort: new AbortController(),
    log: () => {},
    follow: { hooks: () => ({}) as never, timing: host.timing },
  });
  host.run = live;
  let reply: Reply = failing("after-success: same");
  (host as { call: Host["call"] }).call = ((args: string[]) => reply(args)) as Host["call"];
  await refreshMentions(host, live);
  reply = ok;
  await refreshMentions(host, live);
  reply = failing("after-success: same");
  await refreshMentions(host, live);
  assert.equal(pi.errors().length, 1, "same error, a success, the same error: shown once (§15.1)");
});
