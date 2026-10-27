import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { Refusal, type CallTool } from "../lib/slack-channel.ts";
import { addReaction, main } from "../lib/slack-react.ts";

const CHANNEL = "C02JU6C2Z1D";
const TS = "1793040197.490209";
const EMOJI = "git-merge";
const here = dirname(fileURLToPath(import.meta.url));
const helper = join(here, "..", "lib", "slack-react.ts");
const ARGS = ["--coding-agent", "/x/index.js", "--channel", CHANNEL, "--ts", TS, "--emoji", EMOJI];
const ENV = { PI_CODING_AGENT_DIR: "/agent" };

function fake(answer: string | Refusal) {
  const calls: Array<{ name: string; args: Record<string, unknown>; timeoutMs: number }> = [];
  const call: CallTool = async (name, args, timeoutMs) => {
    calls.push({ name, args, timeoutMs });
    if (answer instanceof Refusal) throw answer;
    return answer;
  };
  return { call, calls };
}

test("adds the emoji to the message with slack_add_reaction and answers already false", async () => {
  const f = fake("ok");
  const out = await addReaction(f.call, CHANNEL, TS, EMOJI);
  assert.deepEqual(f.calls, [{ name: "slack_add_reaction", args: { channel_id: CHANNEL, message_ts: TS, emoji: EMOJI }, timeoutMs: 30000 }]);
  assert.deepEqual(out, { channel: CHANNEL, ts: TS, emoji: EMOJI, already: false });
});

test("an already_reacted error is a success with already true", async () => {
  const f = fake(new Refusal("slack_failed", "An API error occurred: already_reacted"));
  const out = await addReaction(f.call, CHANNEL, TS, EMOJI);
  assert.deepEqual(out, { channel: CHANNEL, ts: TS, emoji: EMOJI, already: true });
});

test("any other failed call is the Refusal slack_failed of the call, unchanged", async () => {
  const failure = new Refusal("slack_failed", "boom", { a: 1 });
  await assert.rejects(addReaction(fake(failure).call, CHANNEL, TS, EMOJI), (e: Refusal) => {
    assert.equal(e, failure);
    return true;
  });
});

test("main prints ok with channel, ts, emoji and already and closes the connection", async () => {
  const f = fake("ok");
  let closed = 0;
  const open = (async () => ({ call: f.call, close: async () => void closed++ })) as any;
  const r = await main(ARGS, ENV, "/cwd", open);
  assert.deepEqual(r, { value: { ok: true, channel: CHANNEL, ts: TS, emoji: EMOJI, already: false }, exit: 0 });
  assert.equal(closed, 1);
});

test("main refuses a channel, a ts or an emoji of a wrong shape before connecting", async () => {
  let opened = 0;
  const open = (async () => void opened++) as any;
  const cases: Array<[string, string, string]> = [
    ["c02ju6c2z1d", TS, EMOJI],
    [CHANNEL, "1793040197490209", EMOJI],
    [CHANNEL, "1793040197", EMOJI],
    [CHANNEL, TS, ":git-merge:"],
    [CHANNEL, TS, "Git"],
    [CHANNEL, TS, ""],
  ];
  for (const [channel, ts, emoji] of cases) {
    const r = await main(["--coding-agent", "/x/index.js", "--channel", channel, "--ts", ts, "--emoji", emoji], ENV, "/cwd", open);
    assert.equal(r.exit, 2);
    assert.equal((r.value as any).ok, false);
    assert.equal((r.value as any).error.code, "usage");
  }
  assert.equal(opened, 0);
});

test("main refuses a missing PI_CODING_AGENT_DIR and a missing argument", async () => {
  const open = (async () => { throw new Error("must not connect"); }) as any;
  const noEmoji = ["--coding-agent", "/x/index.js", "--channel", CHANNEL, "--ts", TS];
  for (const [argv, env] of [[ARGS, {}], [noEmoji, ENV]] as const) {
    const r = await main([...argv], env, "/cwd", open);
    assert.equal(r.exit, 2);
    assert.equal((r.value as any).error.code, "usage");
  }
});

test("main prints the refusal of the connection as ok false with its code and exit", async () => {
  const open = (async () => { throw new Refusal("slack_auth", "run pi mcp login slack"); }) as any;
  const r = await main(ARGS, ENV, "/cwd", open);
  assert.equal(r.exit, 3);
  assert.equal((r.value as any).ok, false);
  assert.equal((r.value as any).error.code, "slack_auth");
});

test("run as a script it prints one JSON line", () => {
  const r = spawnSync(process.execPath, [helper, "--coding-agent", "/nonexistent/pi/index.js", "--channel", CHANNEL, "--ts", TS, "--emoji", EMOJI], {
    env: { ...process.env, PI_CODING_AGENT_DIR: "/nonexistent/agent" },
    encoding: "utf8",
    timeout: 30_000,
  });
  assert.equal(r.status, 1, r.stderr);
  const lines = r.stdout.trim().split("\n");
  assert.equal(lines.length, 1);
  const out = JSON.parse(lines[0]!);
  assert.equal(out.ok, false);
  assert.equal(out.error.code, "slack_helper_failed");
});

const REFUSED = ["invalid_name", "bad_timestamp", "message_not_found", "channel_not_found", "is_archived", "thread_locked", "not_reactable", "too_many_emoji", "too_many_reactions", "no_item_specified"];
const failedWith = (name: string) => new Refusal("slack_failed", `slack_add_reaction returned an error: execution_failed: ${name}`);

test("addReaction maps each permanent Slack refusal to slack_refused with its reason", async () => {
  for (const name of REFUSED) {
    await assert.rejects(addReaction(fake(failedWith(name)).call, CHANNEL, TS, EMOJI), (e: Refusal) => {
      assert.equal(e.code, "slack_refused", name);
      assert.equal((e.details as any).reason, name);
      return true;
    });
  }
});

test("addReaction keeps slack_failed for transient and unknown Slack errors", async () => {
  for (const name of ["ratelimited", "internal_error", "missing_scope", "not_in_channel", "timed out after 30000ms", "no slack name here", "not_invalid_named"]) {
    await assert.rejects(addReaction(fake(failedWith(name)).call, CHANNEL, TS, EMOJI), (e: Refusal) => {
      assert.equal(e.code, "slack_failed", name);
      return true;
    });
  }
  const out = await addReaction(fake(failedWith("already_reacted")).call, CHANNEL, TS, EMOJI);
  assert.equal(out.already, true);
});

test("main prints slack_refused with the reason for invalid_name", async () => {
  const f = fake(failedWith("invalid_name"));
  const open = (async () => ({ call: f.call, close: async () => {} })) as any;
  const r = await main(ARGS, ENV, "/cwd", open);
  assert.equal(r.exit, 1);
  const v = r.value as any;
  assert.equal(v.ok, false);
  assert.equal(v.error.code, "slack_refused");
  assert.equal(v.error.details.reason, "invalid_name");
});
