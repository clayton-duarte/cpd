import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { Refusal, type CallTool } from "../lib/slack-channel.ts";
import { main, postMessage } from "../lib/slack-post.ts";

const CHANNEL = "C02JU6C2Z1D";
const TS = "1793040197.490209";
const LINK = "https://missionlane.slack.com/archives/C02JU6C2Z1D/p1793040197490209";
const REAL = JSON.stringify({ message_link: LINK, message_context: { message_ts: TS, channel_id: CHANNEL } });
const here = dirname(fileURLToPath(import.meta.url));
const helper = join(here, "..", "lib", "slack-post.ts");

function fake(answer: string | Refusal) {
  const calls: Array<{ name: string; args: Record<string, unknown>; timeoutMs: number }> = [];
  const call: CallTool = async (name, args, timeoutMs) => {
    calls.push({ name, args, timeoutMs });
    if (answer instanceof Refusal) throw answer;
    return answer;
  };
  return { call, calls };
}

test("sends the message to the channel with the app link preview off and answers channel, ts and link", async () => {
  const f = fake(REAL);
  const out = await postMessage(f.call, CHANNEL, "Title\nhttps://github.com/a/b/pull/1");
  assert.deepEqual(f.calls, [{ name: "slack_send_message", args: { channel_id: CHANNEL, message: "Title\nhttps://github.com/a/b/pull/1", unfurl_app_links: false }, timeoutMs: 30000 }]);
  assert.deepEqual(out, { channel: CHANNEL, ts: TS, link: LINK });
});

test("an answer that is not the message JSON is slack_unparseable and says the message may have been posted", async () => {
  await assert.rejects(postMessage(fake("Sent!").call, CHANNEL, "hi"), (e: Refusal) => {
    assert.equal(e.code, "slack_unparseable");
    assert.equal(e.exit, 1);
    assert.match(e.message, /may have been posted/);
    return true;
  });
});

test("an answer for another channel is slack_unparseable", async () => {
  const other = JSON.stringify({ message_link: LINK, message_context: { message_ts: TS, channel_id: "C0OTHER0000" } });
  await assert.rejects(postMessage(fake(other).call, CHANNEL, "hi"), { code: "slack_unparseable" });
});

test("a failed send is the Refusal slack_failed of the call, unchanged", async () => {
  const failure = new Refusal("slack_failed", "boom", { a: 1 });
  await assert.rejects(postMessage(fake(failure).call, CHANNEL, "hi"), (e: Refusal) => {
    assert.equal(e, failure);
    return true;
  });
});

test("main prints ok with channel, ts and link and closes the connection", async () => {
  const f = fake(REAL);
  let closed = 0;
  const open = (async () => ({ call: f.call, close: async () => void closed++ })) as any;
  const r = await main(["--coding-agent", "/x/index.js", "--channel", CHANNEL], { PI_CODING_AGENT_DIR: "/agent" }, "/cwd", "hello", open);
  assert.deepEqual(r, { value: { ok: true, channel: CHANNEL, ts: TS, link: LINK }, exit: 0 });
  assert.equal(f.calls[0]!.args.message, "hello");
  assert.equal(closed, 1);
});

test("main refuses an empty message and a message over 5000 characters before connecting", async () => {
  let opened = 0;
  const open = (async () => void opened++) as any;
  for (const stdin of ["", "x".repeat(5001)]) {
    const r = await main(["--coding-agent", "/x/index.js", "--channel", CHANNEL], { PI_CODING_AGENT_DIR: "/agent" }, "/cwd", stdin, open);
    assert.equal(r.exit, 2);
    assert.equal((r.value as any).ok, false);
    assert.equal((r.value as any).error.code, "usage");
  }
  assert.equal(opened, 0);
});

test("main refuses a channel that is not a Slack id and a missing PI_CODING_AGENT_DIR", async () => {
  const open = (async () => { throw new Error("must not connect"); }) as any;
  for (const [channel, env] of [["general", { PI_CODING_AGENT_DIR: "/agent" }], [CHANNEL, {}]] as const) {
    const r = await main(["--coding-agent", "/x/index.js", "--channel", channel], env, "/cwd", "hello", open);
    assert.equal(r.exit, 2);
    assert.equal((r.value as any).error.code, "usage");
  }
});

test("main prints the refusal of the connection as ok false with its code and exit", async () => {
  const open = (async () => { throw new Refusal("slack_auth", "run pi mcp login slack"); }) as any;
  const r = await main(["--coding-agent", "/x/index.js", "--channel", CHANNEL], { PI_CODING_AGENT_DIR: "/agent" }, "/cwd", "hello", open);
  assert.equal(r.exit, 3);
  assert.equal((r.value as any).ok, false);
  assert.equal((r.value as any).error.code, "slack_auth");
});

test("run as a script it reads the message from stdin and prints one JSON line", () => {
  const r = spawnSync(process.execPath, [helper, "--coding-agent", "/nonexistent/pi/index.js", "--channel", CHANNEL], {
    env: { ...process.env, PI_CODING_AGENT_DIR: "/nonexistent/agent" },
    input: "hello",
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

const ARGV = ["--coding-agent", "/x/index.js", "--channel", CHANNEL];
const ENV = { PI_CODING_AGENT_DIR: "/agent" };
async function run(extra: string[]) {
  const f = fake(REAL);
  const open = (async () => ({ call: f.call, close: async () => {} })) as any;
  const r = await main([...ARGV, ...extra], ENV, "/cwd", "Bibbidi-bobbidi-bump!", open);
  return { f, r };
}

test("a thread reply sends thread_ts", async () => {
  const { f, r } = await run(["--thread-ts", TS]);
  assert.equal(r.exit, 0);
  assert.deepEqual(f.calls[0]!.args, { channel_id: CHANNEL, message: "Bibbidi-bobbidi-bump!", unfurl_app_links: false, thread_ts: TS });
});

test("reply broadcast true sends reply_broadcast true with the thread", async () => {
  const { f, r } = await run(["--thread-ts", TS, "--reply-broadcast", "true"]);
  assert.equal(r.exit, 0);
  assert.deepEqual(f.calls[0]!.args, { channel_id: CHANNEL, message: "Bibbidi-bobbidi-bump!", unfurl_app_links: false, thread_ts: TS, reply_broadcast: true });
});

test("reply broadcast false sends no reply_broadcast", async () => {
  const { f, r } = await run(["--thread-ts", TS, "--reply-broadcast", "false"]);
  assert.equal(r.exit, 0);
  assert.deepEqual(f.calls[0]!.args, { channel_id: CHANNEL, message: "Bibbidi-bobbidi-bump!", unfurl_app_links: false, thread_ts: TS });
});

test("reply broadcast without thread ts is a usage error", async () => {
  const { f, r } = await run(["--reply-broadcast", "true"]);
  assert.equal(r.exit, 2);
  assert.equal((r.value as any).error.code, "usage");
  assert.equal(f.calls.length, 0);
});

test("a reply broadcast value other than true or false is a usage error", async () => {
  const { f, r } = await run(["--thread-ts", TS, "--reply-broadcast", "yes"]);
  assert.equal(r.exit, 2);
  assert.equal((r.value as any).error.code, "usage");
  assert.equal(f.calls.length, 0);
});

test("without the thread flags the call is unchanged", async () => {
  const f = fake(REAL);
  await postMessage(f.call, CHANNEL, "hi", {});
  assert.deepEqual(f.calls, [{ name: "slack_send_message", args: { channel_id: CHANNEL, message: "hi", unfurl_app_links: false }, timeoutMs: 30000 }]);
});
